#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::test_support;
use super::{snapshot_protected_location, verify_unchanged};
use std::fs;

#[test]
fn verify_unchanged_reports_a_missing_target_without_recreating_it() {
    let (root, target, location, original) =
        test_support::fixture("hardgate-restore", "verify-missing", b"original\n");
    fs::remove_file(&target).unwrap();

    let error = verify_unchanged(&location, &original).unwrap_err();

    test_support::assert_error(&error, std::io::ErrorKind::NotFound, "source disappeared");
    assert!(!target.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn snapshot_protected_location_reports_a_missing_target() {
    let root =
        crate::engines::mutation::test_support::temp_root("hardgate-restore", "snapshot-missing");
    let target = root.join("fixture.rs");

    let error = match snapshot_protected_location(&target, &root) {
        Ok(_) => panic!("missing protected target unexpectedly accepted"),
        Err(error) => error,
    };

    test_support::assert_error(&error, std::io::ErrorKind::NotFound, "does not exist");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restore_mutation_location_accepts_an_unchanged_original_without_writing() {
    let (root, target, location, original) =
        test_support::fixture("hardgate-restore", "mutation-noop", b"original\n");

    super::restore_mutation_location(&location, &original, &original).unwrap();

    assert_eq!(fs::read(&target).unwrap(), original.bytes);
    let _ = fs::remove_dir_all(root);
}
