use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

const TEMP_ATTEMPTS: u64 = 32;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn ensure_regular_file(path: &Path) -> io::Result<Permissions> {
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
    original_bytes: &[u8],
    permissions: &Permissions,
) -> io::Result<()> {
    ensure_restore_target(path)?;
    atomic_replace(path, original_bytes, permissions)?;
    ensure_regular_file(path)?;
    let restored = fs::read(path)?;
    if restored == original_bytes {
        Ok(())
    } else {
        Err(io::Error::other(
            "restored source bytes differ from the original",
        ))
    }
}

fn ensure_restore_target(path: &Path) -> io::Result<()> {
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
    bytes: &[u8],
    permissions: &Permissions,
) -> io::Result<()> {
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
    bytes: &'a [u8],
    permissions: &'a Permissions,
}

fn write_temp_and_rename(mut temp: File, write: AtomicWrite<'_>) -> io::Result<()> {
    write_exact(&mut temp, write.bytes)?;
    temp.set_permissions(write.permissions.clone())?;
    temp.sync_all()?;
    drop(temp);
    fs::rename(write.temp_path, write.path)?;
    let _ = ensure_regular_file(write.path)?;
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

fn path_display(parent: &Path, name: &str) -> String {
    parent.join(name).display().to_string()
}
