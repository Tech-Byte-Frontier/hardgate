use std::path::Path;
use std::process::Command;

pub fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(target, content).unwrap();
}

pub fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

pub fn init_repo(root: &Path) {
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "hardgate@example.invalid"][..],
        &["config", "user.name", "Hardgate Test"][..],
        &["config", "commit.gpgsign", "false"][..],
    ] {
        git(root, args);
    }
}

pub fn commit_baseline(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", message]);
}
