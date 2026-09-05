use super::{COPY_BUFFER_SIZE, copy_tree, verify_copy};
use crate::engines::mutation::test_support::temp_root;
use std::fs;
use std::path::{Path, PathBuf};

fn fixture_roots(tag: &str) -> (PathBuf, PathBuf) {
    let source = temp_root("hardgate-copy-source", tag);
    let destination = temp_root("hardgate-copy-destination", tag);
    (source, destination)
}

fn remove_fixture_roots(source: &Path, destination: &Path) {
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(destination);
}

fn binary_bytes(length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| (index as u8).wrapping_mul(37).wrapping_add(11))
        .collect()
}

#[test]
fn copy_tree_streams_binary_files_and_keeps_source_unchanged() {
    let (source, destination) = fixture_roots("workspace-copy-chunks");
    let nested = source.join("nested");
    fs::create_dir(&nested).unwrap();
    let source_bytes = binary_bytes(COPY_BUFFER_SIZE * 2 + 17);
    let nested_bytes = binary_bytes(COPY_BUFFER_SIZE - 1);
    fs::write(source.join("binary.bin"), &source_bytes).unwrap();
    fs::write(nested.join("boundary.bin"), &nested_bytes).unwrap();
    fs::write(source.join("empty.bin"), b"").unwrap();

    copy_tree(
        &source.canonicalize().unwrap(),
        &destination.canonicalize().unwrap(),
    )
    .unwrap();

    assert_eq!(
        fs::read(destination.join("binary.bin")).unwrap(),
        source_bytes
    );
    assert_eq!(
        fs::read(destination.join("nested/boundary.bin")).unwrap(),
        nested_bytes
    );
    assert_eq!(fs::read(source.join("binary.bin")).unwrap(), source_bytes);
    assert_eq!(
        fs::read(source.join("nested/boundary.bin")).unwrap(),
        nested_bytes
    );
    assert_eq!(fs::read(destination.join("empty.bin")).unwrap(), b"");
    assert_eq!(fs::read(source.join("empty.bin")).unwrap(), b"");

    let mut copied = fs::read(destination.join("binary.bin")).unwrap();
    copied[0] ^= 0xff;
    fs::write(destination.join("binary.bin"), copied).unwrap();
    assert_eq!(fs::read(source.join("binary.bin")).unwrap(), source_bytes);

    remove_fixture_roots(&source, &destination);
}

#[test]
fn verify_copy_handles_boundaries_and_rejects_size_or_last_byte_changes() {
    let (source, destination) = fixture_roots("workspace-copy-verify");
    let origin = source.join("origin.bin");
    let copied = destination.join("copied.bin");

    for length in [
        0,
        1,
        COPY_BUFFER_SIZE - 1,
        COPY_BUFFER_SIZE,
        COPY_BUFFER_SIZE + 1,
    ] {
        let bytes = binary_bytes(length);
        fs::write(&origin, &bytes).unwrap();
        fs::write(&copied, &bytes).unwrap();
        let before = fs::symlink_metadata(&origin).unwrap();
        verify_copy(&origin, &copied, &before).unwrap();

        if length > 0 {
            let mut corrupted = bytes.clone();
            let last = corrupted.last_mut().unwrap();
            *last ^= 0xff;
            fs::write(&copied, &corrupted).unwrap();
            assert!(verify_copy(&origin, &copied, &before).is_err());

            fs::write(&copied, &bytes[..length - 1]).unwrap();
            assert!(verify_copy(&origin, &copied, &before).is_err());
        }

        let mut extended = bytes;
        extended.push(0xaa);
        fs::write(&copied, extended).unwrap();
        assert!(verify_copy(&origin, &copied, &before).is_err());
    }

    remove_fixture_roots(&source, &destination);
}

#[cfg(unix)]
#[test]
fn copy_tree_preserves_executable_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (source, destination) = fixture_roots("workspace-copy-permissions");
    let executable = source.join("run.sh");
    fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o751)).unwrap();

    copy_tree(
        &source.canonicalize().unwrap(),
        &destination.canonicalize().unwrap(),
    )
    .unwrap();

    let copied = destination.join("run.sh");
    assert_eq!(
        fs::metadata(copied).unwrap().permissions().mode() & 0o7777,
        0o751
    );

    remove_fixture_roots(&source, &destination);
}

#[cfg(unix)]
#[test]
fn copy_tree_rehomes_workspace_symlinks() {
    use std::os::unix::fs::symlink;

    let (source, destination) = fixture_roots("workspace-copy-links");
    let nested = source.join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("payload.bin"), b"payload").unwrap();
    symlink("nested/payload.bin", source.join("payload-link")).unwrap();

    copy_tree(
        &source.canonicalize().unwrap(),
        &destination.canonicalize().unwrap(),
    )
    .unwrap();

    let copied_link = destination.join("payload-link");
    assert!(
        fs::symlink_metadata(&copied_link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_link(&copied_link).unwrap(),
        destination.join("nested/payload.bin")
    );
    assert_eq!(fs::read(&copied_link).unwrap(), b"payload");

    remove_fixture_roots(&source, &destination);
}
