#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::create_temp_file;
use crate::engines::mutation::test_support::temp_root;
use std::ffi::OsStr;
use std::fs::{self, File};

#[test]
fn create_temp_file_reports_a_non_directory_parent() {
    let root = temp_root("hardgate-temp", "non-directory-parent");
    let target = root.join("fixture.rs");
    fs::write(&target, b"original\n").unwrap();
    let parent = File::open(&target).unwrap();

    let error = create_temp_file(&parent, OsStr::new("fixture.rs")).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
    let _ = fs::remove_dir_all(root);
}
