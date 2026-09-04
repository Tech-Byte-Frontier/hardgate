#[path = "support/fs.rs"]
mod fs;

use hardgate::git_evidence::{ReferenceEvidence, load_reference, touches};
use std::path::{Path, PathBuf};
use std::process::Command;

struct Repo(PathBuf);

impl Repo {
    fn new(tag: &str) -> Self {
        let root = fs::tempdir(tag);
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "hardgate@example.invalid"]);
        git(&root, &["config", "user.name", "Hardgate Test"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        Self(root)
    }

    fn write(&self, path: &str, contents: &str) {
        let target = self.0.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, contents).unwrap();
    }

    fn commit(&self, message: &str) {
        git(&self.0, &["add", "-A"]);
        git(&self.0, &["commit", "-qm", message]);
    }
}

impl std::ops::Deref for Repo {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn assert_changed_lines(evidence: &ReferenceEvidence, path: &Path, expected: &[usize]) {
    let actual: std::collections::BTreeSet<_> = expected.iter().copied().collect();
    assert_eq!(evidence.change_set.changed_lines[path], actual);
}

fn committed_repo(tag: &str, path: &str, contents: &str) -> Repo {
    let repo = Repo::new(tag);
    repo.write(path, contents);
    repo.commit("base");
    repo
}

#[test]
fn resolves_merge_base_for_reference_branch() {
    let repo = Repo::new("git-evidence-merge-base");
    repo.write("src/lib.rs", "pub fn answer() -> i32 { 1 }\n");
    repo.commit("base");
    git(&repo, &["branch", "reference"]);
    repo.write("src/lib.rs", "pub fn answer() -> i32 { 2 }\n");
    repo.commit("head");

    let evidence = load_reference(&repo, "reference").unwrap();
    assert_eq!(evidence.change_set.merge_base, evidence.snapshot.commit);
    assert_eq!(
        evidence.snapshot.files[Path::new("src/lib.rs")],
        "pub fn answer() -> i32 { 1 }\n"
    );
}

#[test]
fn attributes_modified_hunks_to_post_image_lines() {
    let repo = committed_repo("git-evidence-hunks", "src/lib.rs", "one\ntwo\nthree\n");
    let path = Path::new("src/lib.rs");
    repo.write("src/lib.rs", "one\nchanged\nthree\nadded\n");

    let evidence = load_reference(&repo, "HEAD").unwrap();
    assert_changed_lines(&evidence, path, &[2, 4]);
    assert!(touches(&evidence.change_set.changed_lines, path, 2, 2));
    assert!(!touches(&evidence.change_set.changed_lines, path, 3, 3));
    assert!(evidence.change_set.changed_files.contains(path));
}

#[test]
fn includes_each_line_of_untracked_inventory_files() {
    let repo = committed_repo("git-evidence-untracked", "src/lib.rs", "base\n");
    let path = Path::new("src/new.rs");
    repo.write("src/new.rs", "one\ntwo\nthree\n");
    repo.write("vendor/generated.rs", "must be skipped\n");

    let evidence = load_reference(&repo, "HEAD").unwrap();
    assert_changed_lines(&evidence, path, &[1, 2, 3]);
    assert!(evidence.change_set.changed_files.contains(path));
    assert!(
        !evidence
            .change_set
            .changed_files
            .contains(Path::new("vendor/generated.rs"))
    );
}

#[test]
fn tracks_empty_added_inventory_file() {
    let repo = committed_repo("git-evidence-empty-added", "src/lib.rs", "base\n");
    let path = Path::new("src/empty.rs");
    repo.write("src/empty.rs", "");
    git(&repo, &["add", "src/empty.rs"]);

    let evidence = load_reference(&repo, "HEAD").unwrap();
    assert!(evidence.change_set.changed_files.contains(path));
    assert!(!evidence.change_set.changed_lines.contains_key(path));
}

#[test]
fn handles_renames_and_paths_with_spaces() {
    let repo = Repo::new("git-evidence-rename-space");
    repo.write("src/old name.rs", "first\nsecond\nthird\nfourth\n");
    repo.commit("base");
    git(&repo, &["mv", "src/old name.rs", "src/new name.rs"]);
    repo.write("src/new name.rs", "first\nchanged\nthird\nfourth\n");

    let evidence = load_reference(&repo, "HEAD").unwrap();
    let new_path = Path::new("src/new name.rs");
    assert_changed_lines(&evidence, new_path, &[1, 2, 3, 4]);
    assert!(evidence.change_set.changed_files.contains(new_path));
    assert_eq!(
        evidence.snapshot.files[Path::new("src/old name.rs")],
        "first\nsecond\nthird\nfourth\n"
    );
    assert_eq!(
        evidence
            .change_set
            .rename_lineage
            .get(new_path)
            .map(PathBuf::as_path),
        Some(Path::new("src/old name.rs"))
    );
}

#[test]
fn handles_git_quoted_paths() {
    let repo = Repo::new("git-evidence-quoted-path");
    let old = "src/old.rs";
    let new = "src/new\tname.rs";
    repo.write(old, "before\n");
    repo.commit("base");
    git(&repo, &["mv", old, new]);
    repo.write(new, "after\n");

    let evidence = load_reference(&repo, "HEAD").unwrap();
    assert!(evidence.change_set.changed_files.contains(Path::new(new)));
    assert_changed_lines(&evidence, Path::new(new), &[1]);
}

#[test]
fn unavailable_reference_fails_closed() {
    let repo = Repo::new("git-evidence-missing-reference");
    repo.write("src/lib.rs", "base\n");
    repo.commit("base");
    let error = load_reference(&repo, "does-not-exist").unwrap_err();
    assert!(error.to_string().contains("merge-base") || error.to_string().contains("failed"));
}

#[test]
fn snapshot_reads_utf8_inventory_blobs_and_skips_build_trees() {
    let repo = Repo::new("git-evidence-snapshot");
    repo.write("src/lib.rs", "pub fn value() -> i32 { 7 }\n");
    repo.write("config.json", "{\"ok\":true}\n");
    repo.write("vendor/dependency.rs", "not project code\n");
    repo.write("build/generated.rs", "not project code\n");
    repo.write("README.txt", "not inventoried\n");
    repo.commit("base");

    let evidence = load_reference(&repo, "HEAD").unwrap();
    assert_eq!(
        evidence.snapshot.files[Path::new("src/lib.rs")],
        "pub fn value() -> i32 { 7 }\n"
    );
    assert_eq!(
        evidence.snapshot.files[Path::new("config.json")],
        "{\"ok\":true}\n"
    );
    assert!(
        !evidence
            .snapshot
            .files
            .contains_key(Path::new("vendor/dependency.rs"))
    );
    assert!(
        !evidence
            .snapshot
            .files
            .contains_key(Path::new("build/generated.rs"))
    );
    assert!(
        !evidence
            .snapshot
            .files
            .contains_key(Path::new("README.txt"))
    );
}
