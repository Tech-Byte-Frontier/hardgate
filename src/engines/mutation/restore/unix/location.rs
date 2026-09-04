use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

pub struct TargetLocation {
    pub(super) root: File,
    pub(super) root_identity: DirectoryIdentity,
    pub(super) parent: File,
    pub(super) parent_identity: DirectoryIdentity,
    pub(super) name: OsString,
    pub(super) display: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

impl TargetLocation {
    pub(super) fn open(path: &Path, root: &Path) -> io::Result<Self> {
        let (canonical_root, relative) = contained_relative_path(path, root)?;
        let root_fd = open_root(&canonical_root)?;
        let root_identity = directory_identity(&root_fd)?;
        let name = relative.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "mutation target '{}' has no valid file name",
                    path.display()
                ),
            )
        })?;
        let parent = open_parent(&root_fd, &relative, path)?;
        let parent_identity = directory_identity(&parent)?;
        Ok(Self {
            root: root_fd,
            root_identity,
            parent,
            parent_identity,
            name: name.to_os_string(),
            display: path.to_path_buf(),
        })
    }
}

/// Reopen the live spelling solely for identity validation. All writes still
/// use the original held parent descriptor; this check prevents a detached or
/// replaced parent from being reported as a successful restoration.
pub fn verify_live_location(location: &TargetLocation, path: &Path, root: &Path) -> io::Result<()> {
    let live = TargetLocation::open(path, root)?;
    if live.root_identity != location.root_identity {
        return Err(io::Error::other(
            "repository root identity changed during mutation",
        ));
    }
    if live.parent_identity != location.parent_identity {
        return Err(io::Error::other(
            "mutation target parent identity changed during mutation",
        ));
    }
    Ok(())
}

fn open_root(path: &Path) -> io::Result<File> {
    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::DIRECTORY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::CLOEXEC;
    #[cfg(target_os = "linux")]
    {
        // Start from the trusted filesystem root. `canonicalize` is only a
        // spelling resolver; no path component is trusted until openat2 has
        // checked it with BENEATH|NO_SYMLINKS|NO_MAGICLINKS.
        let anchor: File = rustix::fs::open(Path::new("/"), flags, rustix::fs::Mode::empty())
            .map_err(io::Error::from)?
            .into();
        let relative = path.strip_prefix(Path::new("/")).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("canonical root '{}' is not absolute", path.display()),
            )
        })?;
        let relative = if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        };
        openat2_checked(&anchor, relative, flags)
            .map_err(|error| descriptor_open_error(path, path, error))
    }
    #[cfg(target_os = "macos")]
    {
        let anchor = rustix::fs::open(Path::new("/"), flags, rustix::fs::Mode::empty())
            .map_err(io::Error::from)?;
        walk_directory(anchor.into(), path, flags, path)
    }
}

fn open_parent(root_fd: &File, relative: &Path, path: &Path) -> io::Result<File> {
    #[cfg(target_os = "linux")]
    {
        let parent_path = relative
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let flags = rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::CLOEXEC;
        openat2_checked(root_fd, parent_path, flags)
            .map_err(|error| descriptor_open_error(path, parent_path, error))
    }
    #[cfg(target_os = "macos")]
    {
        let parent_path = relative.parent().unwrap_or_else(|| Path::new("."));
        let flags = rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC;
        walk_directory(root_fd.try_clone()?, parent_path, flags, path)
    }
}

fn openat2_checked(
    base: &File,
    relative: &Path,
    flags: rustix::fs::OFlags,
) -> Result<File, rustix::io::Errno> {
    rustix::fs::openat2(
        base,
        relative,
        flags,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::BENEATH
            | rustix::fs::ResolveFlags::NO_SYMLINKS
            | rustix::fs::ResolveFlags::NO_MAGICLINKS,
    )
    .map(Into::into)
}

fn descriptor_open_error(path: &Path, part: &Path, error: rustix::io::Errno) -> io::Error {
    if error == rustix::io::Errno::NOSYS {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "openat2 is unavailable; refusing descriptor-relative mutation",
        )
    } else {
        ancestor_error(path, part, error)
    }
}

#[cfg(target_os = "macos")]
fn walk_directory(
    mut current: File,
    components: &Path,
    flags: rustix::fs::OFlags,
    display: &Path,
) -> io::Result<File> {
    for component in components.components() {
        let Component::Normal(part) = component else {
            if matches!(component, Component::RootDir | Component::CurDir) {
                continue;
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "mutation target '{}' has unsafe directory components",
                    display.display()
                ),
            ));
        };
        let fd = rustix::fs::openat(&current, part, flags, rustix::fs::Mode::empty())
            .map_err(|error| ancestor_error(display, Path::new(part), error))?;
        current = fd.into();
    }
    Ok(current)
}

fn contained_relative_path(path: &Path, root: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let canonical_root = fs::canonicalize(root)?;
    let normalized_path = normalize_absolute(path)?;
    let normalized_root = normalize_absolute(root)?;
    let relative = if let Ok(relative) = normalized_path.strip_prefix(&canonical_root) {
        relative.to_path_buf()
    } else if let Ok(relative) = normalized_path.strip_prefix(&normalized_root) {
        relative.to_path_buf()
    } else {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing mutation target '{}' outside repository root '{}'",
                path.display(),
                canonical_root.display()
            ),
        ));
    };
    if relative.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "mutation target '{}' resolves to the repository root",
                path.display()
            ),
        ));
    }
    Ok((canonical_root, relative))
}

fn normalize_absolute(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        append_component(&mut normalized, component, path)?;
    }
    Ok(normalized)
}

fn append_component(
    normalized: &mut PathBuf,
    component: Component<'_>,
    original: &Path,
) -> io::Result<()> {
    match component {
        Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
        Component::RootDir => normalized.push(component.as_os_str()),
        Component::CurDir => {}
        Component::ParentDir if normalized.pop() => {}
        Component::ParentDir => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path '{}' escapes its filesystem root", original.display()),
            ));
        }
        Component::Normal(part) => normalized.push(part),
    }
    Ok(())
}

fn ancestor_error(path: &Path, part: &Path, error: rustix::io::Errno) -> io::Error {
    if error == rustix::io::Errno::LOOP {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing mutation target '{}'; ancestor '{}' is a symlink",
                path.display(),
                part.display()
            ),
        )
    } else {
        io::Error::from(error)
    }
}

fn directory_identity(directory: &File) -> io::Result<DirectoryIdentity> {
    let stat = rustix::fs::fstat(directory).map_err(io::Error::from)?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "descriptor-relative mutation parent is no longer a directory",
        ));
    }
    Ok(DirectoryIdentity {
        device: stat.st_dev as u64,
        inode: stat.st_ino as u64,
    })
}

pub(super) fn verify_descriptor_identity(location: &TargetLocation) -> io::Result<()> {
    let root = directory_identity(&location.root)?;
    if root != location.root_identity {
        return Err(io::Error::other(
            "descriptor-relative mutation repository root identity changed",
        ));
    }
    let current = directory_identity(&location.parent)?;
    if current == location.parent_identity {
        Ok(())
    } else {
        Err(io::Error::other(
            "descriptor-relative mutation parent identity changed",
        ))
    }
}
