use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Fixture {
    pub(super) root: PathBuf,
}

impl Fixture {
    pub(super) fn new(tag: &str) -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("hardgate-memory-{tag}-{}-{id}", std::process::id()));
        fs::create_dir(&root).unwrap();
        Self { root }
    }

    pub(super) fn write(&self, name: &str, value: &str) {
        fs::write(self.root.join(name), value).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) fn invalid<T>(result: io::Result<T>) -> io::Error {
    match result {
        Ok(_) => panic!("fixture should be rejected"),
        Err(error) => error,
    }
}

pub(super) fn invalid_with_context<T>(
    result: io::Result<T>,
    context: impl std::fmt::Display,
) -> io::Error {
    match result {
        Ok(_) => panic!("fixture should be rejected: {context}"),
        Err(error) => error,
    }
}
