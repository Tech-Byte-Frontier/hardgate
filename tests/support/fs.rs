//! Filesystem fixtures for integration tests.

use std::path::PathBuf;

/// Fresh unique temp dir for a test. The caller removes it at the end.
pub fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hardgate-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
