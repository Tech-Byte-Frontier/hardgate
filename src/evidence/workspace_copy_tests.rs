use super::*;
use std::io::Seek;

struct CopyFixture {
    root: PathBuf,
    origin: PathBuf,
    copied: PathBuf,
}

impl CopyFixture {
    fn new(bytes: &[u8]) -> Self {
        let root = crate::fs_tests::tempdir("copy-races")
            .canonicalize()
            .unwrap();
        let origin = root.join("origin");
        let copied = root.join("copied");
        fs::write(&origin, bytes).unwrap();
        fs::write(&copied, bytes).unwrap();
        Self {
            root,
            origin,
            copied,
        }
    }
}

impl Drop for CopyFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn copied_bytes_and_lengths_are_verified_across_buffer_boundaries() {
    let fixture = CopyFixture::new(&vec![42; COPY_BUFFER_SIZE + 17]);
    let before = fs::metadata(&fixture.origin).unwrap();
    assert!(files_match(&fixture.origin, &fixture.copied, &before).unwrap());
    fs::write(&fixture.copied, vec![41; COPY_BUFFER_SIZE + 17]).unwrap();
    assert!(!files_match(&fixture.origin, &fixture.copied, &before).unwrap());
    fs::write(&fixture.copied, b"short").unwrap();
    assert!(!files_match(&fixture.origin, &fixture.copied, &before).unwrap());
    fs::write(&fixture.origin, b"short").unwrap();
    assert!(!files_match(&fixture.origin, &fixture.copied, &before).unwrap());
    let empty = CopyFixture::new(b"");
    assert!(
        files_match(
            &empty.origin,
            &empty.copied,
            &fs::metadata(&empty.origin).unwrap()
        )
        .unwrap()
    );
}

#[test]
fn source_replacement_and_same_size_edits_invalidate_a_copy() {
    let fixture = CopyFixture::new(b"original");
    let before = fs::metadata(&fixture.origin).unwrap();
    fs::write(&fixture.copied, b"modified").unwrap();
    assert!(
        verify_copy(&fixture.origin, &fixture.copied, &before)
            .unwrap_err()
            .to_string()
            .contains("source bytes changed")
    );
    fs::File::options()
        .write(true)
        .open(&fixture.origin)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
        .unwrap();
    assert!(verify_copy(&fixture.origin, &fixture.copied, &before).is_err());
    fs::write(&fixture.origin, b"short").unwrap();
    assert!(verify_copy(&fixture.origin, &fixture.copied, &before).is_err());
    fs::remove_file(&fixture.origin).unwrap();
    fs::create_dir(&fixture.origin).unwrap();
    assert!(verify_copy(&fixture.origin, &fixture.copied, &before).is_err());
}

#[test]
fn changes_after_reading_are_detected_before_accepting_the_snapshot() {
    for change in [
        "grow-origin",
        "grow-copy",
        "replace-origin",
        "replace-copy",
        "shrink-origin",
        "shrink-copy",
        "retime-origin",
    ] {
        let fixture = CopyFixture::new(b"original");
        let before = fs::metadata(&fixture.origin).unwrap();
        let mut origin = fs::File::open(&fixture.origin).unwrap();
        let mut copied = fs::File::open(&fixture.copied).unwrap();
        origin.seek(std::io::SeekFrom::End(0)).unwrap();
        copied.seek(std::io::SeekFrom::End(0)).unwrap();
        let changed = if change.ends_with("origin") {
            &fixture.origin
        } else {
            &fixture.copied
        };
        match change.split('-').next().unwrap() {
            "grow" => fs::OpenOptions::new()
                .append(true)
                .open(changed)
                .unwrap()
                .write_all(b"!")
                .unwrap(),
            "replace" => {
                fs::remove_file(changed).unwrap();
                fs::create_dir(changed).unwrap();
            }
            "shrink" => fs::write(changed, b"short").unwrap(),
            "retime" => fs::File::options()
                .write(true)
                .open(changed)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
                .unwrap(),
            _ => unreachable!(),
        }
        let context = FileEndContext {
            origin: &fixture.origin,
            copied: &fixture.copied,
            before: &before,
        };
        assert!(
            !verify_file_end_state(&mut origin, &mut copied, &context).unwrap(),
            "{change}"
        );
    }
}

#[cfg(unix)]
#[test]
fn special_files_and_links_into_omitted_data_are_refused() {
    let fixture = CopyFixture::new(b"original");
    let destination = fixture.root.join("destination");
    fs::create_dir(&destination).unwrap();
    #[cfg(target_os = "linux")]
    let directory = fs::File::open(&fixture.root).unwrap();
    #[cfg(target_os = "linux")]
    let address = PathBuf::from(format!(
        "/proc/self/fd/{}/socket",
        std::os::fd::AsRawFd::as_raw_fd(&directory)
    ));
    #[cfg(not(target_os = "linux"))]
    let address = fixture.root.join("socket");
    let _socket = std::os::unix::net::UnixListener::bind(address).unwrap();
    assert!(
        copy_entry(&fixture.root, &destination, Path::new("socket"),)
            .unwrap_err()
            .to_string()
            .contains("special file")
    );
    fs::create_dir(fixture.root.join("target")).unwrap();
    fs::write(fixture.root.join("target/artifact"), b"build output").unwrap();
    std::os::unix::fs::symlink(
        fixture.root.join("target/artifact"),
        fixture.root.join("link"),
    )
    .unwrap();
    assert!(
        copy_link(&fixture.root, &destination, Path::new("link"))
            .unwrap_err()
            .to_string()
            .contains("omitted")
    );
}

#[test]
fn parallel_copy_preserves_all_bytes_and_keeps_writes_independent() {
    let fixture = CopyFixture::new(b"original");
    let source = fixture.root.join("source");
    let copied = fixture.root.join("snapshot");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&copied).unwrap();
    for index in 0..128 {
        fs::write(
            source.join(index.to_string()),
            vec![index as u8; COPY_BUFFER_SIZE + 1],
        )
        .unwrap();
    }
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .stack_size(256 * 1024)
        .build()
        .unwrap()
        .install(|| copy_tree(&source, &copied))
        .unwrap();
    assert_eq!(fs::read_dir(&copied).unwrap().count(), 128);
    for index in 0..128 {
        let name = index.to_string();
        assert_eq!(
            fs::read(copied.join(&name)).unwrap(),
            fs::read(source.join(&name)).unwrap()
        );
        fs::write(copied.join(&name), b"mutated").unwrap();
        assert_eq!(
            fs::metadata(source.join(name)).unwrap().len(),
            (COPY_BUFFER_SIZE + 1) as u64
        );
    }
}
