//! Deny child filesystem writes outside its disposable workspace and Cargo cache.
//! Requires Landlock ABI 3: older ABIs cannot prevent truncation of source files.
use std::fs::{self, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const WRITE_FILE: u64 = 1 << 1;
const TRUNCATE: u64 = 1 << 14;
// ABI 1 creation/removal rights, ABI 2 REFER, and ABI 3 TRUNCATE.
const WRITE_TREE: u64 = WRITE_FILE | (((1 << 15) - 1) & !((1 << 4) - 1));

#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
}

#[repr(C, packed)]
struct PathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

pub(super) fn configure(command: &mut Command, copy: &Path, original: &Path) -> io::Result<()> {
    let copy = copy.canonicalize()?;
    let original = original.canonicalize()?;
    require_disjoint(&copy, &original)?;
    let ruleset = create_ruleset()?;
    // A descendant may run a mutation producer. Prepare its validated shared
    // lease now; the sandbox must never grant writes to the global lock path.
    crate::resources::prepare_mutation_lease()?;
    allow(&ruleset, &copy)?;
    if let Some(cache) = cargo_home().filter(|path| path.is_dir()) {
        let cache = cache.canonicalize()?;
        require_disjoint(&cache, &original)?;
        allow(&ruleset, &cache)?;
        command.env("CARGO_HOME", cache);
    }
    allow(&ruleset, Path::new("/dev/null"))?;
    let scratch = crate::evidence::temporary::scratch_directory(&copy);
    fs::create_dir_all(&scratch)?;
    command
        .env("TMPDIR", &scratch)
        .env("TMP", &scratch)
        .env("TEMP", &scratch)
        .env("XDG_CACHE_HOME", scratch.join("cache"))
        .env("UV_CACHE_DIR", scratch.join("uv"))
        .env("npm_config_cache", scratch.join("npm"))
        .env("npm_config_store_dir", scratch.join("pnpm-store"))
        .env("pnpm_config_store_dir", scratch.join("pnpm-store"))
        .env("pnpm_config_verify_deps_before_run", "error");
    // Only async-signal-safe syscalls run after fork. The descriptor stays alive
    // in the closure until exec; Landlock restrictions are inherited by children.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}

fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
}

fn require_disjoint(writable: &Path, original: &Path) -> io::Result<()> {
    if original.starts_with(writable) || writable.starts_with(original) {
        return Err(io::Error::other(format!(
            "read-only checks need a disposable workspace and Cargo cache outside the checkout; writable path {} overlaps {}",
            writable.display(),
            original.display()
        )));
    }
    Ok(())
}

fn create_ruleset() -> io::Result<OwnedFd> {
    let abi = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<RulesetAttr>(),
            0,
            1,
        )
    };
    if abi < 3 {
        return Err(io::Error::other(
            "read-only checks require Linux with Landlock ABI 3 or newer enabled; use a supported kernel with Landlock in its LSM configuration",
        ));
    }
    let attr = RulesetAttr {
        handled_access_fs: WRITE_TREE,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &attr,
            size_of::<RulesetAttr>(),
            0,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // A successful syscall returns a new descriptor owned by this scope.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

fn allow(ruleset: &OwnedFd, path: &Path) -> io::Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_CLOEXEC)
        .open(path)?;
    let attr = PathBeneathAttr {
        allowed_access: if file.metadata()?.is_dir() {
            WRITE_TREE
        } else {
            WRITE_FILE | TRUNCATE
        },
        parent_fd: file.as_raw_fd(),
    };
    if unsafe {
        libc::syscall(
            libc::SYS_landlock_add_rule,
            ruleset.as_raw_fd(),
            1,
            &attr,
            0,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
