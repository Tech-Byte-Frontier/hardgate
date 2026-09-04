use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const TEMP_ATTEMPTS: u64 = 32;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub(super) struct SourceSnapshot {
    pub(super) bytes: Vec<u8>,
    pub(super) permissions: Permissions,
}

pub(super) fn snapshot_regular_file(path: &Path, root: &Path) -> io::Result<SourceSnapshot> {
    let permissions = ensure_regular_file(path, root)?;
    let bytes = fs::read(path)?;
    Ok(SourceSnapshot { bytes, permissions })
}

pub(super) fn ensure_regular_file(path: &Path, root: &Path) -> io::Result<Permissions> {
    ensure_safe_ancestors(path, root)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to write through symbolic link target '{}'; target is a symlink",
                path.display()
            ),
        ));
    }
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to restore non-regular mutation target '{}'; target is not a regular file",
                path.display()
            ),
        ));
    }
    Ok(metadata.permissions())
}

pub(super) fn restore_and_verify(
    path: &Path,
    root: &Path,
    original_bytes: &[u8],
    permissions: &Permissions,
) -> io::Result<()> {
    ensure_restore_target(path, root)?;
    atomic_replace(path, root, original_bytes, permissions)?;
    let restored_permissions = ensure_regular_file(path, root)?;
    if !same_permissions(permissions, &restored_permissions) {
        return Err(io::Error::other(
            "restored source permissions differ from the original",
        ));
    }
    let restored = fs::read(path)?;
    if restored == original_bytes {
        Ok(())
    } else {
        Err(io::Error::other(
            "restored source bytes differ from the original",
        ))
    }
}

pub(super) fn verify_and_restore(
    path: &Path,
    root: &Path,
    original: &SourceSnapshot,
) -> io::Result<bool> {
    let changed = match snapshot_regular_file(path, root) {
        Ok(current) => {
            !same_permissions(&current.permissions, &original.permissions)
                || current.bytes != original.bytes
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => return Err(error),
    };
    if changed {
        restore_and_verify(path, root, &original.bytes, &original.permissions)?;
    }
    Ok(changed)
}

fn ensure_restore_target(path: &Path, root: &Path) -> io::Result<()> {
    ensure_safe_ancestors(path, root)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to write through symbolic link target '{}'; target is a symlink",
                path.display()
            ),
        )),
        Ok(metadata) if !metadata.file_type().is_file() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to restore non-regular mutation target '{}'; target is not a regular file",
                path.display()
            ),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn atomic_replace(
    path: &Path,
    root: &Path,
    bytes: &[u8],
    permissions: &Permissions,
) -> io::Result<()> {
    ensure_safe_ancestors(path, root)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "mutation target '{}' has no valid file name",
                    path.display()
                ),
            )
        })?;
    let (temp_path, temp) = create_temp_file(parent, name)?;
    let result = write_temp_and_rename(
        temp,
        AtomicWrite {
            temp_path: &temp_path,
            path,
            root,
            bytes,
            permissions,
        },
    );
    cleanup_temp(&temp_path, result)
}

fn create_temp_file(parent: &Path, name: &str) -> io::Result<(std::path::PathBuf, File)> {
    let seed = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    for offset in 0..TEMP_ATTEMPTS {
        let candidate = parent.join(format!(
            ".{name}.hardgate-{}-{}.tmp",
            std::process::id(),
            seed.saturating_add(offset)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "could not allocate a temporary mutation file beside '{}'; tried {TEMP_ATTEMPTS} names",
            path_display(parent, name)
        ),
    ))
}

struct AtomicWrite<'a> {
    temp_path: &'a Path,
    path: &'a Path,
    root: &'a Path,
    bytes: &'a [u8],
    permissions: &'a Permissions,
}

fn write_temp_and_rename(mut temp: File, write: AtomicWrite<'_>) -> io::Result<()> {
    write_exact(&mut temp, write.bytes)?;
    temp.set_permissions(write.permissions.clone())?;
    temp.sync_all()?;
    drop(temp);
    fs::rename(write.temp_path, write.path)?;
    let written_permissions = ensure_regular_file(write.path, write.root)?;
    if !same_permissions(write.permissions, &written_permissions) {
        return Err(io::Error::other(
            "atomic replacement permissions differ from requested source permissions",
        ));
    }
    let written = fs::read(write.path)?;
    if written == write.bytes {
        Ok(())
    } else {
        Err(io::Error::other(
            "atomic replacement bytes differ from requested source bytes",
        ))
    }
}

fn write_exact(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.flush()
}

fn cleanup_temp<T>(temp_path: &Path, result: io::Result<T>) -> io::Result<T> {
    if result.is_ok() {
        return result;
    }
    let cleanup = fs::remove_file(temp_path);
    let Err(error) = result else {
        unreachable!("successful replacement returned before temporary cleanup")
    };
    match cleanup {
        Ok(()) => Err(error),
        Err(cleanup) if cleanup.kind() == io::ErrorKind::NotFound => Err(error),
        Err(cleanup) => Err(io::Error::other(format!(
            "{error}; failed to clean temporary mutation file '{}': {cleanup}",
            temp_path.display()
        ))),
    }
}

fn ensure_safe_ancestors(path: &Path, root: &Path) -> io::Result<()> {
    let root = fs::canonicalize(root)?;
    let path = normalize_absolute(path)?;
    let relative = path.strip_prefix(&root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing mutation target '{}' outside repository root '{}'",
                path.display(),
                root.display()
            ),
        )
    })?;
    let relative_parent = relative
        .parent()
        .filter(|item| !item.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new(""));
    let mut current = root;
    for component in relative_parent.components() {
        let Component::Normal(part) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "mutation target '{}' has unsafe ancestor components",
                    path.display()
                ),
            ));
        };
        current.push(part);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "refusing mutation target '{}'; ancestor '{}' is a symlink",
                    path.display(),
                    current.display()
                ),
            ));
        }
        if !metadata.file_type().is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "refusing mutation target '{}'; ancestor '{}' is not a directory",
                    path.display(),
                    current.display()
                ),
            ));
        }
    }
    Ok(())
}

fn normalize_absolute(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        normalize_component(&mut normalized, component, path)?;
    }
    Ok(normalized)
}

fn normalize_component(
    normalized: &mut PathBuf,
    component: Component<'_>,
    original: &Path,
) -> io::Result<()> {
    match component {
        Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
        Component::RootDir => normalized.push(component.as_os_str()),
        Component::CurDir => {}
        Component::ParentDir => pop_component(normalized, original)?,
        Component::Normal(part) => normalized.push(part),
    }
    Ok(())
}

fn pop_component(normalized: &mut PathBuf, original: &Path) -> io::Result<()> {
    if normalized.pop() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path '{}' escapes its filesystem root", original.display()),
        ))
    }
}

fn same_permissions(expected: &Permissions, actual: &Permissions) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        expected.mode() == actual.mode()
    }
    #[cfg(not(unix))]
    {
        expected.readonly() == actual.readonly()
    }
}

fn path_display(parent: &Path, name: &str) -> String {
    parent.join(name).display().to_string()
}
