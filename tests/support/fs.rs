//! Filesystem fixtures for integration tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Fresh unique temp dir for a test. The caller removes it at the end.
pub fn tempdir(tag: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("hardgate-test-{tag}-{}-{id}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                // Bound discovery; Git tests initialize this empty marker.
                std::fs::create_dir(dir.join(".git")).unwrap();
                return dir;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot create test fixture {}: {error}", dir.display()),
        }
    }
}
