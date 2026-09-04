#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::{RestoreLocation, SourceSnapshot, open_location, snapshot_location};
use crate::engines::mutation::test_support::temp_root;
use std::fs;
use std::io;
use std::path::PathBuf;

pub(crate) fn fixture(
    prefix: &str,
    label: &str,
    bytes: &[u8],
) -> (PathBuf, PathBuf, RestoreLocation, SourceSnapshot) {
    let root = temp_root(prefix, label);
    let target = root.join("fixture.rs");
    fs::write(&target, bytes).unwrap();
    let location = open_location(&target, &root).unwrap();
    let original = snapshot_location(&location).unwrap().unwrap();
    (root, target, location, original)
}

pub(crate) fn assert_error(error: &io::Error, kind: io::ErrorKind, message: &str) {
    assert_eq!(error.kind(), kind);
    assert_message(error, message);
}

pub(crate) fn assert_message(error: &io::Error, message: &str) {
    assert!(error.to_string().contains(message));
}
