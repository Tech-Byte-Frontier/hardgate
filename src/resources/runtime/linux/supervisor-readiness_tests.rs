use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixture() -> Readiness {
    Readiness::create(&format!(
        "test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
    .unwrap()
}

#[test]
fn readiness_is_private_reusable_and_removes_only_its_owned_files() {
    let ready = fixture();
    let directory = ready.0.parent().unwrap().to_owned();
    assert!(!ready.observed());
    acknowledge_path(&ready.0).unwrap();
    assert!(ready.observed());
    acknowledge_path(&ready.0).unwrap();
    assert_eq!(fs::metadata(&ready.0).unwrap().mode() & 0o777, 0o600);
    fs::write(directory.join("unrelated"), "preserve").unwrap();
    drop(ready);
    assert!(!directory.join("ready").exists());
    assert_eq!(
        fs::read_to_string(directory.join("unrelated")).unwrap(),
        "preserve"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn readiness_rejects_foreign_names_public_directories_and_symlinks() {
    let ready = fixture();
    let parent = ready.0.parent().unwrap();
    assert!(acknowledge_path(Path::new("/")).is_err());
    assert!(acknowledge_path(&parent.join("wrong-name")).is_err());
    fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(acknowledge_path(&ready.0).is_err());
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    let link = parent.with_extension("link");
    symlink(parent, &link).unwrap();
    assert!(acknowledge_path(&link.join("ready")).is_err());
    fs::remove_file(link).unwrap();
    let other = parent.join("foreign");
    fs::create_dir(&other).unwrap();
    assert!(acknowledge_path(&other.join("ready")).is_err());
    fs::remove_dir(other).unwrap();
}

#[test]
fn inherited_readiness_rejects_links_modes_and_foreign_ownership() {
    let ready = fixture();
    acknowledge_path(&ready.0).unwrap();
    let uid = rustix::process::getuid().as_raw();
    assert!(validate_existing(&ready.0, uid.wrapping_add(1)).is_err());
    fs::set_permissions(&ready.0, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(acknowledge_path(&ready.0).is_err());
    fs::set_permissions(&ready.0, fs::Permissions::from_mode(0o600)).unwrap();
    let alias = ready.0.with_extension("alias");
    fs::hard_link(&ready.0, &alias).unwrap();
    assert!(acknowledge_path(&ready.0).is_err());
    fs::remove_file(alias).unwrap();
    fs::remove_file(&ready.0).unwrap();
    symlink("absent", &ready.0).unwrap();
    assert!(acknowledge_path(&ready.0).is_err());
    fs::remove_file(&ready.0).unwrap();
    fs::create_dir(&ready.0).unwrap();
    assert!(acknowledge_path(&ready.0).is_err());
    fs::remove_dir(&ready.0).unwrap();
    assert!(validate_existing(&ready.0, uid).is_err());
}
