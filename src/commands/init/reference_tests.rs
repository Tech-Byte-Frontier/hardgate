use super::*;
use std::fs;

#[test]
fn missing_repository_reference_is_distinguished_from_invalid_path() {
    let root = std::env::temp_dir().join(format!("hardgate-init-reference-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    assert_eq!(
        legacy_reference_status(&root, "origin/main"),
        ReferenceStatus::Missing
    );
    fs::remove_dir_all(root).unwrap();

    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let invalid = OsString::from_vec(vec![0xff]);
        assert_eq!(
            legacy_reference_status(std::path::Path::new(&invalid), "origin/main"),
            ReferenceStatus::Unknown
        );
    }
}
