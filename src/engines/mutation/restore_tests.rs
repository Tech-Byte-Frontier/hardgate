#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::{
    restore_mutation_location, snapshot_location, snapshot_protected_location, verify_unchanged,
};
use crate::engines::mutation::test_support::temp_root;
use std::fs;

#[test]
fn verify_unchanged_reports_a_missing_target_without_recreating_it() {
    let root = temp_root("hardgate-restore", "verify-missing");
    let target = root.join("fixture.rs");
    fs::write(&target, b"original\n").unwrap();
    let location = super::open_location(&target, &root).unwrap();
    let original = snapshot_location(&location).unwrap().unwrap();
    fs::remove_file(&target).unwrap();

    let error = verify_unchanged(&location, &original).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(error.to_string().contains("source disappeared"));
    assert!(!target.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn snapshot_protected_location_reports_a_missing_target() {
    let root = temp_root("hardgate-restore", "snapshot-missing");
    let target = root.join("fixture.rs");

    let error = snapshot_protected_location(&target, &root).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(error.to_string().contains("does not exist"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restore_mutation_location_accepts_an_unchanged_original_without_writing() {
    let root = temp_root("hardgate-restore", "mutation-noop");
    let target = root.join("fixture.rs");
    let original_bytes = b"original\n";
    fs::write(&target, original_bytes).unwrap();
    let location = super::open_location(&target, &root).unwrap();
    let original = snapshot_location(&location).unwrap().unwrap();

    super::restore_mutation_location(&location, &original, &original).unwrap();

    assert_eq!(fs::read(&target).unwrap(), original_bytes);
    let _ = fs::remove_dir_all(root);
}
