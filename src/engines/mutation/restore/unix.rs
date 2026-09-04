use super::{FileIdentity, SourceSnapshot, same_permissions, same_snapshot_identity};
#[path = "unix/temp.rs"]
mod temp;

use std::fs::{File, Permissions};
use std::io::{self, Read, Write};
use std::path::Path;

#[path = "unix/location.rs"]
mod location;
use location::verify_descriptor_identity;

#[derive(Clone, Copy)]
pub(super) struct LocationContext<'a> {
    location: &'a TargetLocation,
    path: &'a Path,
    root: &'a Path,
}

impl<'a> LocationContext<'a> {
    pub(super) fn new(location: &'a TargetLocation, path: &'a Path, root: &'a Path) -> Self {
        Self {
            location,
            path,
            root,
        }
    }
}
pub(super) use location::{TargetLocation, verify_live_location};

pub(super) fn open_location(path: &Path, root: &Path) -> io::Result<TargetLocation> {
    TargetLocation::open(path, root)
}

pub(super) fn snapshot_location(location: &TargetLocation) -> io::Result<Option<SourceSnapshot>> {
    match read_location(location) {
        Ok(current) => Ok(Some(current.snapshot)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn restore_location(
    context: LocationContext<'_>,
    original: &SourceSnapshot,
    expected_mutation: Option<&SourceSnapshot>,
) -> io::Result<()> {
    let location = context.location;
    verify_descriptor_identity(location)?;
    verify_live_location(location, context.path, context.root)?;
    reject_existing_target(location)?;
    verify_expected(location, expected_mutation)?;
    atomic_replace_at(
        context,
        &original.bytes,
        &original.permissions,
        expected_mutation,
    )
}

pub(super) fn atomic_replace_location(
    context: LocationContext<'_>,
    bytes: &[u8],
    permissions: &Permissions,
    expected: Option<&SourceSnapshot>,
) -> io::Result<()> {
    atomic_replace_at(context, bytes, permissions, expected)
}

fn reject_existing_target(location: &TargetLocation) -> io::Result<()> {
    match rustix::fs::statat(
        &location.parent,
        &location.name,
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
    ) {
        Ok(stat) => validate_target_type(location, stat.st_mode),
        Err(error) => missing_target_or_error(error),
    }
}

fn validate_target_type(location: &TargetLocation, mode: rustix::fs::RawMode) -> io::Result<()> {
    match rustix::fs::FileType::from_raw_mode(mode) {
        rustix::fs::FileType::RegularFile => Ok(()),
        rustix::fs::FileType::Symlink => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to write through symbolic link target '{}'; target is a symlink",
                location.display.display()
            ),
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to restore non-regular mutation target '{}'; target is not a regular file",
                location.display.display()
            ),
        )),
    }
}

fn missing_target_or_error(error: rustix::io::Errno) -> io::Result<()> {
    if io::Error::from(error).kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(io::Error::from(error))
    }
}

struct CurrentFile {
    snapshot: SourceSnapshot,
}

fn read_location(location: &TargetLocation) -> io::Result<CurrentFile> {
    let fd = rustix::fs::openat(
        &location.parent,
        &location.name,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let stat = rustix::fs::fstat(&fd).map_err(io::Error::from)?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to restore non-regular mutation target '{}'; target is not a regular file",
                location.display.display()
            ),
        ));
    }
    let mut file: File = fd.into();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let mode = stat.st_mode as u32 & 0o7777;
    Ok(CurrentFile {
        snapshot: SourceSnapshot {
            bytes,
            permissions: permissions_from_mode(mode),
            identity: FileIdentity {
                device: stat.st_dev as u64,
                inode: stat.st_ino as u64,
                links: stat.st_nlink as u64,
                mode: stat.st_mode as u32,
            },
        },
    })
}

fn atomic_replace_at(
    context: LocationContext<'_>,
    bytes: &[u8],
    permissions: &Permissions,
    expected: Option<&SourceSnapshot>,
) -> io::Result<()> {
    let location = context.location;
    let (temp_name, mut temp) = temp::create_temp_file(&location.parent, &location.name)?;
    let result = (|| {
        verify_descriptor_identity(location)?;
        verify_live_location(location, context.path, context.root)?;
        temp.write_all(bytes)?;
        temp.flush()?;
        set_file_permissions(&temp, permissions)?;
        temp.sync_all()?;
        verify_expected(location, expected)?;
        verify_live_location(location, context.path, context.root)?;
        drop(temp);
        rustix::fs::renameat(
            &location.parent,
            &temp_name,
            &location.parent,
            &location.name,
        )
        .map_err(io::Error::from)?;
        rustix::fs::fsync(&location.parent).map_err(io::Error::from)?;
        verify_descriptor_identity(location)?;
        verify_live_location(location, context.path, context.root)?;
        let written = read_location(location)?.snapshot;
        verify_contents(&written, bytes, permissions)
    })();
    if let Err(error) = &result {
        match rustix::fs::unlinkat(&location.parent, &temp_name, rustix::fs::AtFlags::empty()) {
            Ok(()) => {}
            Err(cleanup) if cleanup == rustix::io::Errno::NOENT => {}
            Err(cleanup) => {
                return Err(io::Error::other(format!(
                    "{error}; failed to clean temporary mutation file '{}': {}",
                    temp_name.to_string_lossy(),
                    io::Error::from(cleanup)
                )));
            }
        }
    }
    result
}

fn snapshots_match(current: &SourceSnapshot, expected: &SourceSnapshot) -> bool {
    same_snapshot_identity(current, expected)
        && current.bytes == expected.bytes
        && same_permissions(&current.permissions, &expected.permissions)
}

fn verify_expected(location: &TargetLocation, expected: Option<&SourceSnapshot>) -> io::Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    match snapshot_location(location)? {
        Some(current) if snapshots_match(&current, expected) => Ok(()),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mutation target changed before descriptor-relative replacement",
        )),
        // A command may delete the target between checks. The rename still
        // creates the original entry through the held parent.
        None => Ok(()),
    }
}

fn set_file_permissions(file: &File, permissions: &Permissions) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = permissions.mode() as rustix::fs::RawMode;
    rustix::fs::fchmod(file, rustix::fs::Mode::from_raw_mode(mode)).map_err(io::Error::from)
}

fn permissions_from_mode(mode: u32) -> Permissions {
    use std::os::unix::fs::PermissionsExt;

    Permissions::from_mode(mode)
}

fn verify_contents(
    current: &SourceSnapshot,
    expected_bytes: &[u8],
    expected_permissions: &Permissions,
) -> io::Result<()> {
    if current.bytes != expected_bytes {
        return Err(io::Error::other(
            "atomic replacement bytes differ from requested source bytes",
        ));
    }
    if !same_permissions(&current.permissions, expected_permissions) {
        return Err(io::Error::other(
            "atomic replacement permissions differ from requested source permissions",
        ));
    }
    Ok(())
}
