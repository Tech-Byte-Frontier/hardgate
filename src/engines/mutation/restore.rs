//! Fail-closed source snapshots and atomic replacement.
//!
//! The platform implementation is kept in `restore/unix.rs` so this module
//! stays within the source-size budget. Targets other than Linux/macOS return
//! an explicit unsupported error before any baseline or mutation source write.

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "restore/unix.rs"]
mod unix;

use std::fs::Permissions;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) struct SourceSnapshot {
    pub(super) bytes: Vec<u8>,
    pub(super) permissions: Permissions,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) identity: FileIdentity,
}

pub(super) enum ExpectedEntry<'a> {
    Present(&'a SourceSnapshot),
    Missing,
}

/// Replacement bytes and ownership checks carried through an atomic rename.
/// Keeping this as one value avoids widening the platform seam with a long
/// list of independently ordered arguments.
pub(super) struct AtomicReplacement<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) permissions: &'a Permissions,
    pub(super) expected: ExpectedEntry<'a>,
    pub(super) armed: Option<&'a mut Option<SourceSnapshot>>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FileIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
    pub(super) links: u64,
    /// Full `st_mode`, including file type and permission bits.
    pub(super) mode: u32,
}

/// Open a trusted, descriptor-relative target location. The returned handle
/// keeps the validated repository root and parent directory alive so callers
/// can verify and replace the same directory entry without a second path
/// lookup.
pub(super) fn open_location(path: &Path, root: &Path) -> io::Result<RestoreLocation> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        Ok(RestoreLocation {
            inner: unix::open_location(path, root)?,
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (path, root);
        Err(unsupported_platform_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) struct RestoreLocation {
    inner: unix::TargetLocation,
    path: PathBuf,
    root: PathBuf,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) struct RestoreLocation;

pub(super) fn snapshot_location(location: &RestoreLocation) -> io::Result<Option<SourceSnapshot>> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::snapshot_location(&location.inner)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = location;
        Err(unsupported_platform_error())
    }
}

pub(super) fn verify_live_path(
    location: &RestoreLocation,
    path: &Path,
    root: &Path,
) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::verify_live_location(&location.inner, path, root)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (location, path, root);
        Err(unsupported_platform_error())
    }
}

pub(super) fn restore_location(
    location: &RestoreLocation,
    original: &SourceSnapshot,
    expected: ExpectedEntry<'_>,
) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let context = unix::LocationContext::new(&location.inner, &location.path, &location.root);
        unix::restore_location(context, original, expected)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (location, original, expected);
        Err(unsupported_platform_error())
    }
}

/// Restore after a mutation command while preserving the exact entry observed
/// immediately before the rename. A deleted target is restored only while it
/// remains absent; a live replacement must still match the armed mutation.
pub(super) fn restore_mutation_location(
    location: &RestoreLocation,
    original: &SourceSnapshot,
    armed: &SourceSnapshot,
) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        match verified_snapshot(location)? {
            Some(current) if same_snapshot(&current, original) => Ok(()),
            Some(current) if same_snapshot(&current, armed) => {
                restore_location(location, original, ExpectedEntry::Present(armed))
            }
            Some(_) => Err(io::Error::other(
                "mutation target changed after command execution; refusing to overwrite external edits",
            )),
            None => restore_location(location, original, ExpectedEntry::Missing),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (location, original, armed);
        Err(unsupported_platform_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verified_snapshot(location: &RestoreLocation) -> io::Result<Option<SourceSnapshot>> {
    verify_live_path(location, &location.path, &location.root)?;
    snapshot_location(location)
}

pub(super) fn atomic_replace_location(
    location: &RestoreLocation,
    replacement: AtomicReplacement<'_>,
) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let context = unix::LocationContext::new(&location.inner, &location.path, &location.root);
        unix::atomic_replace_location(context, replacement)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (location, replacement);
        Err(unsupported_platform_error())
    }
}

/// Verify a source without writing it. This is used when mutation application
/// never completed, so an external edit must be reported rather than hidden by
/// a rollback write.
pub(super) fn verify_unchanged(
    location: &RestoreLocation,
    original: &SourceSnapshot,
) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        match verified_snapshot(location)? {
            Some(current) if same_snapshot(&current, original) => Ok(()),
            Some(_) => Err(io::Error::other(
                "source changed before mutation was applied; refusing to overwrite external edits",
            )),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "source disappeared before mutation was applied; refusing to recreate it",
            )),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (location, original);
        Err(unsupported_platform_error())
    }
}

/// Open and snapshot a protected baseline target, retaining the descriptor
/// location so later checks cannot silently follow a replaced parent.
pub(super) fn snapshot_protected_location(
    path: &Path,
    root: &Path,
) -> io::Result<(RestoreLocation, SourceSnapshot)> {
    let location = open_location(path, root)?;
    verify_live_path(&location, path, root)?;
    let snapshot = snapshot_location(&location)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "protected mutation target '{}' does not exist",
                path.display()
            ),
        )
    })?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if snapshot.identity.links > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing baseline: protected source '{}' has {} hardlinks; mutation requires an unshared regular file",
                path.display(),
                snapshot.identity.links
            ),
        ));
    }
    Ok((location, snapshot))
}

pub(super) fn verify_and_restore(
    location: &RestoreLocation,
    path: &Path,
    root: &Path,
    original: &SourceSnapshot,
) -> io::Result<bool> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        verify_live_path(location, path, root)?;
        match snapshot_location(location)? {
            Some(current) if same_snapshot(&current, original) => Ok(false),
            Some(current) => {
                restore_location(location, original, ExpectedEntry::Present(&current))?;
                Ok(true)
            }
            None => {
                restore_location(location, original, ExpectedEntry::Missing)?;
                Ok(true)
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (location, path, root, original);
        Err(unsupported_platform_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn same_snapshot(left: &SourceSnapshot, right: &SourceSnapshot) -> bool {
    same_snapshot_identity(left, right)
        && same_permissions(&left.permissions, &right.permissions)
        && left.bytes == right.bytes
}

pub(super) fn same_permissions(expected: &Permissions, actual: &Permissions) -> bool {
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn same_snapshot_identity(left: &SourceSnapshot, right: &SourceSnapshot) -> bool {
    left.identity == right.identity
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn has_multiple_links(snapshot: &SourceSnapshot) -> bool {
    snapshot.identity.links > 1
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn same_snapshot_identity(_left: &SourceSnapshot, _right: &SourceSnapshot) -> bool {
    false
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn has_multiple_links(_snapshot: &SourceSnapshot) -> bool {
    false
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported_platform_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "mutation source writes and descendant cleanup require Linux or macOS; this platform is unsupported and no source write was attempted",
    )
}

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "restore/test_support_tests.rs"]
pub(crate) mod test_support;
