use super::*;

#[test]
fn edited_receipt_and_report_cannot_match_original_execution_authentication() {
    let root = crate::fs_tests::tempdir("receipt-authentication");
    let original = br#"{"report_sha256":"original"}"#;
    let path = root.join(digest(original));
    fs::write(&path, original).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    verify_at(&root, original).unwrap();
    assert!(verify_at(&root, br#"{"report_sha256":"forged"}"#).is_err());
    fs::write(&path, b"edited registry").unwrap();
    assert!(verify_at(&root, original).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn authentication_rejects_symlinks_and_nonprivate_records() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = crate::fs_tests::tempdir("receipt-alias");
    let bytes = b"execution";
    let original = root.join("original");
    fs::write(&original, bytes).unwrap();
    let path = root.join(digest(bytes));
    symlink(&original, &path).unwrap();
    assert!(verify_at(&root, bytes).is_err());
    fs::remove_file(&path).unwrap();
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(verify_at(&root, bytes).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn interrupted_certification_revokes_authentication_and_committed_runs_can_be_revoked() {
    let root = crate::fs_tests::tempdir("authentication-rollback");
    let report = root.join("report.lcov");
    let bytes = format!("execution at {}", report.display());
    let certificate = certify(bytes.as_bytes(), &root, &root.join("work"), &report).unwrap();
    let record = certificate.path.clone();
    let marker = certificate.slot.clone();
    verify(bytes.as_bytes(), &root, &report).unwrap();
    drop(certificate);
    assert!(!record.exists());
    assert!(!marker.exists());
    assert!(verify(bytes.as_bytes(), &root, &report).is_err());
    certify(bytes.as_bytes(), &root, &root.join("work"), &report)
        .unwrap()
        .commit();
    verify(bytes.as_bytes(), &root, &report).unwrap();
    revoke(&report, &root).unwrap();
    assert!(verify(bytes.as_bytes(), &root, &report).is_err());
    fs::remove_file(record).unwrap();
    fs::remove_dir_all(root).unwrap();
}
