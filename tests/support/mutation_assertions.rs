//! Shared assertion helpers for mutation runner filesystem tests.

use std::path::Path;

pub fn assert_file_contents(path: &Path, expected: &[u8]) {
    assert_eq!(std::fs::read(path).unwrap().as_slice(), expected);
}

pub fn assert_symlink(path: &Path) {
    let metadata = std::fs::symlink_metadata(path).unwrap();
    assert!(metadata.file_type().is_symlink());
}

pub fn remove_dirs(first: &Path, second: &Path) {
    let _ = std::fs::remove_dir_all(first);
    let _ = std::fs::remove_dir_all(second);
}
