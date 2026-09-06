use std::io;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const CHILD_MARKER: &str = "HARDGATE_MUTATION_CHILD";

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::cell::RefCell;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::{self, DirBuilder, File, Metadata, TryLockError};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::rc::{Rc, Weak};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::thread;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::{Duration, Instant};

#[cfg(any(target_os = "linux", target_os = "macos"))]
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(any(target_os = "linux", target_os = "macos"))]
const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct LeaseInner {
    _file: File,
    path: PathBuf,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
thread_local! {
    static HELD_LEASE: RefCell<Weak<LeaseInner>> = const { RefCell::new(Weak::new()) };
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) struct MutationLease {
    _inner: Rc<LeaseInner>,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) struct MutationLease;

impl MutationLease {
    pub(crate) fn acquire() -> io::Result<Self> {
        acquire()
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn acquire() -> io::Result<MutationLease> {
    let path = lock_path();
    let uid = current_uid();
    acquire_at(
        &path,
        uid,
        Instant::now() + LOCK_WAIT_TIMEOUT,
        std::env::var_os(CHILD_MARKER).is_some(),
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn acquire_at(
    path: &Path,
    uid: u32,
    deadline: Instant,
    child_marker: bool,
) -> io::Result<MutationLease> {
    if child_marker {
        return Err(io::Error::other(
            "nested mutation is unsupported: HARDGATE_MUTATION_CHILD is set",
        ));
    }
    crate::cancellation::check()?;

    if let Some(inner) = HELD_LEASE.with(|held| {
        held.borrow()
            .upgrade()
            .filter(|inner| inner.path.as_path() == path)
    }) {
        return Ok(MutationLease { _inner: inner });
    }

    let file = prepare_at(path, uid)?;
    lock_file(&file, path, deadline)?;

    let inner = Rc::new(LeaseInner {
        _file: file,
        path: path.to_path_buf(),
    });
    HELD_LEASE.with(|held| *held.borrow_mut() = Rc::downgrade(&inner));
    Ok(MutationLease { _inner: inner })
}

/// Create and validate the shared lock before child write restrictions apply.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn prepare() -> io::Result<()> {
    prepare_at(&lock_path(), current_uid()).map(drop)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn prepare_at(path: &Path, uid: u32) -> io::Result<File> {
    ensure_lock_directory(path, uid)?;
    validate_existing_lock_path(path, uid)?;
    let file = open_lock_file(path)?;
    validate_lock_metadata(
        &file
            .metadata()
            .map_err(|error| filesystem_error("inspect lock file", error))?,
        uid,
    )?;
    Ok(file)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn acquire() -> io::Result<MutationLease> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "mutation locking is unsupported on this platform",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn lock_path() -> PathBuf {
    PathBuf::from(format!(
        "/tmp/hardgate-mutation-{}/slot.lock",
        current_uid()
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn ensure_lock_directory(path: &Path, uid: u32) -> io::Result<()> {
    let directory = path
        .parent()
        .expect("the deterministic mutation lock path has a parent");
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    match builder.create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(filesystem_error("create lock directory", error)),
    }

    let metadata = fs::symlink_metadata(directory)
        .map_err(|error| filesystem_error("inspect lock directory", error))?;
    if metadata.file_type().is_symlink() {
        return Err(unsafe_path_error("lock directory must not be a symlink"));
    }
    if !metadata.file_type().is_dir() {
        return Err(unsafe_path_error("lock directory is not a directory"));
    }
    if metadata.uid() != uid {
        return Err(unsafe_path_error("lock directory is owned by another user"));
    }
    if metadata.mode() & 0o7777 != 0o700 {
        return Err(unsafe_path_error("lock directory permissions must be 0700"));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_existing_lock_path(path: &Path, uid: u32) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_lock_metadata(&metadata, uid),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(filesystem_error("inspect lock path", error)),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_lock_metadata(metadata: &Metadata, uid: u32) -> io::Result<()> {
    if metadata.file_type().is_symlink() {
        return Err(unsafe_path_error("lock file must not be a symlink"));
    }
    if !metadata.file_type().is_file() {
        return Err(unsafe_path_error("lock path is not a regular file"));
    }
    if metadata.uid() != uid {
        return Err(unsafe_path_error("lock file is owned by another user"));
    }
    if metadata.nlink() != 1 {
        return Err(unsafe_path_error("lock file must not have hard links"));
    }
    if metadata.mode() & 0o7777 != 0o600 {
        return Err(unsafe_path_error("lock file permissions must be 0600"));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_lock_file(path: &Path) -> io::Result<File> {
    // flock needs the descriptor, not write access to the file contents.
    // Reusing the shared lock must work inside a source-write sandbox too.
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::from_raw_mode(0o600),
    )
    .map_err(|error| filesystem_error("open lock file", error))?;
    Ok(File::from(descriptor))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn lock_file(file: &File, path: &Path, deadline: Instant) -> io::Result<()> {
    loop {
        if try_lock_or_wait(file, path, deadline)? {
            return Ok(());
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn try_lock_or_wait(file: &File, path: &Path, deadline: Instant) -> io::Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(TryLockError::WouldBlock) => {
            crate::cancellation::check()?;
            let now = Instant::now();
            if now >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "mutation lock busy after {} seconds: {}",
                        LOCK_WAIT_TIMEOUT.as_secs(),
                        path.display()
                    ),
                ));
            }
            thread::sleep(LOCK_POLL_INTERVAL.min(deadline - now));
            Ok(false)
        }
        Err(TryLockError::Error(error)) => Err(filesystem_error("lock mutation resource", error)),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn unsafe_path_error(detail: &str) -> io::Error {
    io::Error::other(format!(
        "mutation lock filesystem error: unsafe path: {detail}"
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn filesystem_error(operation: &str, error: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!(
        "mutation lock filesystem error while {operation}: {error}"
    ))
}

#[cfg(test)]
#[path = "lease_tests.rs"]
mod tests;

#[cfg(target_os = "linux")]
pub(super) fn acquire_workload() -> io::Result<MutationLease> {
    let uid = current_uid();
    let path = PathBuf::from(format!("/tmp/hardgate-workload-{uid}/slot.lock"));
    acquire_at(&path, uid, Instant::now() + LOCK_WAIT_TIMEOUT, false).map_err(|cause| {
        crate::resources::runtime::error(format!(
            "cannot acquire the per-user workload slot: {cause}"
        ))
    })
}
