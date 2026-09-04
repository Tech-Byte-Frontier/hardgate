#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::super::super::test_support;
use super::{
    TargetLocation, ancestor_error, append_component, contained_relative_path, directory_identity,
    normalize_absolute, verify_descriptor_identity, verify_live_location,
};
use crate::engines::mutation::test_support::temp_root;
use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::path::{Component, Path, PathBuf};

#[test]
fn contained_relative_path_accepts_a_normalized_root_alias() {
    let actual = temp_root("hardgate-location", "root-alias");
    let target = actual.join("fixture.rs");
    fs::write(&target, b"original\n").unwrap();
    let alias = temp_root("hardgate-location", "root-alias-link");
    fs::remove_dir_all(&alias).unwrap();
    symlink(&actual, &alias).unwrap();
    let alias_target = alias.join("fixture.rs");

    let (canonical, relative) = contained_relative_path(&alias_target, &alias).unwrap();

    assert_eq!(canonical, fs::canonicalize(&actual).unwrap());
    assert_eq!(relative, PathBuf::from("fixture.rs"));
    let _ = fs::remove_file(alias);
    let _ = fs::remove_dir_all(actual);
}

#[test]
fn contained_relative_path_rejects_a_target_that_is_the_repository_root() {
    let root = temp_root("hardgate-location", "root-target");

    let error = contained_relative_path(&root, &root).unwrap_err();

    test_support::assert_error(
        &error,
        std::io::ErrorKind::InvalidInput,
        "resolves to the repository root",
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn normalization_handles_curdir_parent_and_filesystem_root_escape() {
    let mut normalized = PathBuf::from("/tmp/hardgate-location");
    append_component(
        &mut normalized,
        Component::CurDir,
        Path::new("./fixture.rs"),
    )
    .unwrap();
    assert_eq!(normalized, PathBuf::from("/tmp/hardgate-location"));

    append_component(
        &mut normalized,
        Component::ParentDir,
        Path::new("../fixture.rs"),
    )
    .unwrap();
    assert_eq!(normalized, PathBuf::from("/tmp"));

    let error = append_component(
        &mut PathBuf::from("/"),
        Component::ParentDir,
        Path::new("/.."),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("escapes its filesystem root"));

    let normalized_relative = normalize_absolute(Path::new("./fixture.rs")).unwrap();
    assert!(normalized_relative.ends_with("fixture.rs"));
}

#[test]
fn directory_identity_rejects_a_regular_file_descriptor() {
    let root = temp_root("hardgate-location", "identity-file");
    let target = root.join("fixture.rs");
    fs::write(&target, b"original\n").unwrap();
    let file = File::open(&target).unwrap();

    let error = directory_identity(&file).unwrap_err();

    test_support::assert_error(
        &error,
        std::io::ErrorKind::InvalidInput,
        "no longer a directory",
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn live_and_descriptor_identity_checks_fail_closed() {
    let root = temp_root("hardgate-location", "identity-mismatch");
    let target = root.join("fixture.rs");
    fs::write(&target, b"original\n").unwrap();

    let mut root_mismatch = TargetLocation::open(&target, &root).unwrap();
    root_mismatch.root_identity.device ^= 1;
    let error = verify_live_location(&root_mismatch, &target, &root).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("repository root identity changed")
    );

    let mut parent_mismatch = TargetLocation::open(&target, &root).unwrap();
    parent_mismatch.parent_identity.inode ^= 1;
    let error = verify_live_location(&parent_mismatch, &target, &root).unwrap_err();
    assert!(error.to_string().contains("target parent identity changed"));

    let mut descriptor_root_mismatch = TargetLocation::open(&target, &root).unwrap();
    descriptor_root_mismatch.root_identity.device ^= 1;
    let error = verify_descriptor_identity(&descriptor_root_mismatch).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("repository root identity changed")
    );

    let mut descriptor_parent_mismatch = TargetLocation::open(&target, &root).unwrap();
    descriptor_parent_mismatch.parent_identity.inode ^= 1;
    let error = verify_descriptor_identity(&descriptor_parent_mismatch).unwrap_err();
    assert!(error.to_string().contains("parent identity changed"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn ancestor_error_classifies_symlink_and_preserves_other_errors() {
    let symlink_error = ancestor_error(
        Path::new("nested/fixture.rs"),
        Path::new("nested"),
        rustix::io::Errno::LOOP,
    );
    assert_eq!(symlink_error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        symlink_error
            .to_string()
            .contains("ancestor 'nested' is a symlink")
    );

    let missing_error = ancestor_error(
        Path::new("nested/fixture.rs"),
        Path::new("nested"),
        rustix::io::Errno::NOENT,
    );
    assert_eq!(missing_error.kind(), std::io::ErrorKind::NotFound);
}
