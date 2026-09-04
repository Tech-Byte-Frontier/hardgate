#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::super::test_support;
use super::{
    AtomicReplacement, ExpectedEntry, FileIdentity, LocationContext, SourceSnapshot,
    TargetLocation, atomic_replace_at, cleanup_temp_entry, reject_existing_target,
    restore_location, same_temp_identity, snapshot_location, temp, temp_file_identity,
    verify_contents, verify_temp_entry,
};
use crate::engines::mutation::test_support::temp_root;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn fixture(label: &str) -> (PathBuf, PathBuf, TargetLocation, SourceSnapshot) {
    let root = temp_root("hardgate-restore-unix", label);
    let target = root.join("fixture.rs");
    fs::write(&target, b"original\n").unwrap();
    let mut permissions = fs::metadata(&target).unwrap().permissions();
    permissions.set_mode(0o640);
    fs::set_permissions(&target, permissions).unwrap();
    let location = super::open_location(&target, &root).unwrap();
    let original = super::snapshot_location(&location).unwrap().unwrap();
    (root, target, location, original)
}

fn restore_present(context: LocationContext<'_>, original: &SourceSnapshot) -> std::io::Result<()> {
    restore_location(context, original, ExpectedEntry::Present(original))
}

fn restore_armed(
    context: LocationContext<'_>,
    original: &SourceSnapshot,
    armed: &SourceSnapshot,
) -> std::io::Result<()> {
    restore_location(context, original, ExpectedEntry::Present(armed))
}

fn assert_restored(target: &Path, bytes: &[u8]) {
    assert_eq!(fs::read(target).unwrap(), bytes);
    assert_eq!(
        fs::metadata(target).unwrap().permissions().mode() & 0o7777,
        0o640
    );
}

#[test]
fn restore_present_original_is_a_noop() {
    let (root, target, location, original) = fixture("restore-noop");

    restore_present(LocationContext::new(&location, &target, &root), &original).unwrap();

    assert_restored(&target, b"original\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restore_present_missing_target_reports_missing_without_recreating_it() {
    let (root, target, location, original) = fixture("restore-present-missing");
    fs::remove_file(&target).unwrap();

    let error =
        restore_present(LocationContext::new(&location, &target, &root), &original).unwrap_err();

    test_support::assert_error(
        &error,
        std::io::ErrorKind::NotFound,
        "disappeared before descriptor-relative replacement",
    );
    assert!(!target.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn atomic_replace_restores_exact_bytes_and_permissions() {
    let (root, target, location, original) = fixture("exact-restoration");
    let replacement_bytes = b"mutated\n";
    let mut armed = None;

    atomic_replace_at(
        LocationContext::new(&location, &target, &root),
        AtomicReplacement {
            bytes: replacement_bytes,
            permissions: &original.permissions,
            expected: ExpectedEntry::Present(&original),
            armed: Some(&mut armed),
        },
    )
    .unwrap();
    let armed_snapshot = armed.as_ref().unwrap();
    assert_eq!(armed_snapshot.bytes, replacement_bytes);
    assert_eq!(fs::read(&target).unwrap(), replacement_bytes);
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o7777,
        0o640
    );

    restore_armed(
        LocationContext::new(&location, &target, &root),
        &original,
        armed_snapshot,
    )
    .unwrap();
    assert_restored(&target, b"original\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn atomic_replace_cleans_temp_after_expected_entry_mismatch() {
    let (root, target, location, original) = fixture("cleanup-mismatch");
    let mismatch = SourceSnapshot {
        bytes: b"unexpected\n".to_vec(),
        permissions: original.permissions.clone(),
        identity: original.identity,
    };

    let error = atomic_replace_at(
        LocationContext::new(&location, &target, &root),
        AtomicReplacement {
            bytes: b"replacement\n",
            permissions: &original.permissions,
            expected: ExpectedEntry::Present(&mismatch),
            armed: None,
        },
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("changed before descriptor-relative replacement")
    );
    assert_eq!(fs::read(&target).unwrap(), b"original\n");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cleanup_accepts_a_temp_entry_already_missing() {
    let root = temp_root("hardgate-restore-unix", "cleanup-missing");
    let parent = File::open(&root).unwrap();
    let (name, temp_file) = temp::create_temp_file(&parent, OsStr::new("fixture.rs")).unwrap();
    let expected = temp_file_identity(&temp_file).unwrap();
    drop(temp_file);
    fs::remove_file(root.join(Path::new(&name))).unwrap();

    let original = std::io::Error::other("simulated atomic failure");
    cleanup_temp_entry(&parent, &name, &expected, &original).unwrap();

    let _ = fs::remove_dir_all(root);
}

#[test]
fn reject_existing_target_accepts_a_missing_entry() {
    let (root, target, location, _original) = fixture("reject-missing");
    fs::remove_file(&target).unwrap();

    reject_existing_target(&location).unwrap();

    let _ = fs::remove_dir_all(root);
}

#[test]
fn verify_temp_entry_reports_identity_mismatch() {
    let root = temp_root("hardgate-restore-unix", "temp-identity");
    let parent = File::open(&root).unwrap();
    let (name, temp_file) = temp::create_temp_file(&parent, OsStr::new("fixture.rs")).unwrap();
    let (other_name, other_file) =
        temp::create_temp_file(&parent, OsStr::new("fixture.rs")).unwrap();
    let wrong_identity = temp_file_identity(&other_file).unwrap();

    let error = verify_temp_entry(&parent, &name, &wrong_identity).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("entry changed before descriptor-relative rename")
    );
    drop(temp_file);
    drop(other_file);
    fs::remove_file(root.join(Path::new(&name))).unwrap();
    fs::remove_file(root.join(Path::new(&other_name))).unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn same_temp_identity_checks_each_identity_component() {
    let root = temp_root("hardgate-restore-unix", "temp-identity-components");
    let parent = File::open(&root).unwrap();
    let (name, temp_file) = temp::create_temp_file(&parent, OsStr::new("fixture.rs")).unwrap();
    let actual = temp_file_identity(&temp_file).unwrap();

    let mut wrong_device = actual;
    wrong_device.device ^= 1;
    assert!(!same_temp_identity(&wrong_device, &actual));
    let mut wrong_inode = actual;
    wrong_inode.inode ^= 1;
    assert!(!same_temp_identity(&wrong_inode, &actual));
    let peer = root.join("peer.tmp");
    fs::hard_link(root.join(Path::new(&name)), &peer).unwrap();
    let linked = temp_file_identity(&temp_file).unwrap();
    assert_eq!(linked.links, actual.links + 1);
    assert!(!same_temp_identity(&linked, &actual));
    let mut wrong_mode = actual;
    wrong_mode.mode = 0o040000;
    assert!(!same_temp_identity(&wrong_mode, &actual));
    assert!(same_temp_identity(&actual, &actual));

    drop(temp_file);
    fs::remove_file(root.join(Path::new(&name))).unwrap();
    fs::remove_file(peer).unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn temp_file_identity_rejects_a_directory_descriptor() {
    let root = temp_root("hardgate-restore-unix", "temp-directory");
    let directory = File::open(&root).unwrap();

    let error = temp_file_identity(&directory).unwrap_err();

    assert!(error.to_string().contains("not a regular file"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn verify_contents_reports_byte_and_permission_mismatches() {
    let (root, target, _location, original) = fixture("verify-contents");
    let wrong_bytes = SourceSnapshot {
        bytes: b"wrong\n".to_vec(),
        permissions: original.permissions.clone(),
        identity: original.identity,
    };
    let bytes_error =
        verify_contents(&wrong_bytes, &original.bytes, &original.permissions).unwrap_err();
    assert!(bytes_error.to_string().contains("bytes differ"));

    let mut wrong_permissions = original.permissions.clone();
    wrong_permissions.set_mode(original.permissions.mode() ^ 0o001);
    let wrong_mode = SourceSnapshot {
        bytes: original.bytes.clone(),
        permissions: wrong_permissions,
        identity: original.identity,
    };
    let permissions_error =
        verify_contents(&wrong_mode, &original.bytes, &original.permissions).unwrap_err();
    assert!(permissions_error.to_string().contains("permissions differ"));

    assert_eq!(fs::read(&target).unwrap(), original.bytes);
    let _ = fs::remove_dir_all(root);
}
