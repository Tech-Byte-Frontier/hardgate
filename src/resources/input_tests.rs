use super::{admit_bytes, read_source};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct SourceFixture {
    path: PathBuf,
}

impl SourceFixture {
    fn new(bytes: &[u8]) -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("hardgate-source-input-{}-{id}", std::process::id()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .expect("source fixture should be created");
        std::io::Write::write_all(&mut file, bytes).expect("source fixture should be written");
        Self { path }
    }

    fn open(&self) -> File {
        File::open(&self.path).expect("source fixture should be opened")
    }
}

impl Drop for SourceFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn error_from<T>(result: io::Result<T>, message: &str) -> io::Error {
    match result {
        Ok(_) => panic!("{message}"),
        Err(error) => error,
    }
}

#[test]
fn reads_empty_source_at_zero_limit() {
    let fixture = SourceFixture::new(&[]);

    assert!(
        read_source(fixture.open(), 0)
            .expect("empty source should fit a zero-byte limit")
            .is_empty()
    );
}

#[test]
fn reads_binary_source_without_text_assumptions() {
    let source = [0, 1, 2, 127, 128, 254, 255];
    let fixture = SourceFixture::new(&source);

    assert_eq!(
        read_source(fixture.open(), source.len()).expect("binary source should be read"),
        source
    );
}

#[test]
fn reads_exact_chunk_boundary_at_the_limit() {
    let source = vec![0xa5; 64 * 1024];
    let fixture = SourceFixture::new(&source);

    assert_eq!(
        read_source(fixture.open(), source.len()).expect("boundary source should fit"),
        source
    );
}

#[test]
fn rejects_metadata_larger_than_limit_without_source_contents() {
    let fixture = SourceFixture::new(b"PRIVATE-SOURCE-CONTENTS");
    let error = error_from(
        read_source(fixture.open(), 3),
        "oversized source should be rejected",
    );

    assert!(
        error
            .to_string()
            .starts_with("mutation resource guard: source/snapshot limit exceeded:"),
        "{error}"
    );
    assert!(error.to_string().contains("source length"), "{error}");
    assert!(!error.to_string().contains("PRIVATE-SOURCE-CONTENTS"));
}

#[test]
fn rejects_limit_overflow_without_changing_total() {
    let mut total = usize::MAX;
    let error = error_from(
        admit_bytes(&mut total, 1, usize::MAX),
        "byte-count overflow should be rejected",
    );

    assert_eq!(total, usize::MAX);
    assert!(error.to_string().contains("byte count overflow"), "{error}");
}

#[test]
fn rejects_limit_excess_without_changing_total() {
    let mut total = 3;
    let error = error_from(
        admit_bytes(&mut total, 2, 4),
        "bytes beyond the limit should be rejected",
    );

    assert_eq!(total, 3);
    assert!(
        error.to_string().contains("source/snapshot size"),
        "{error}"
    );
}

#[test]
fn admits_bytes_at_limit_and_updates_total() {
    let mut total = 2;

    admit_bytes(&mut total, 3, 5).expect("bytes at the limit should be admitted");

    assert_eq!(total, 5);
}
