use super::*;

fn root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("hardgate-output-{label}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn atomic_output_preserves_existing_temporary_names() {
    let root = root("preserve");
    let target = root.join("report.json");
    let unrelated = target.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&unrelated, "unrelated data").unwrap();
    write_atomic_file(&target, "new report").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "new report");
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "unrelated data");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_atomic_replacement_cleans_only_its_own_temporary_file() {
    let root = root("cleanup");
    let target = root.join("directory");
    fs::create_dir_all(&target).unwrap();
    assert!(write_atomic_file(&target, "new report").is_err());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    assert!(target.is_dir());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn atomic_output_does_not_follow_an_existing_temporary_symlink() {
    let root = root("symlink");
    let target = root.join("report.json");
    let unrelated = root.join("unrelated");
    fs::write(&unrelated, "original").unwrap();
    let link = target.with_extension(format!("tmp.{}", std::process::id()));
    std::os::unix::fs::symlink(&unrelated, &link).unwrap();
    write_atomic_file(&target, "new report").unwrap();
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "original");
    assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    fs::remove_dir_all(root).unwrap();
}
