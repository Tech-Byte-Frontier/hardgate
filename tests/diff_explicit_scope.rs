#[path = "common/cli.rs"]
mod cli;
#[path = "support/fs.rs"]
mod fs;
#[path = "common/fs_git.rs"]
mod fs_git;

use cli::{json, run};
use fs::tempdir;
use fs_git::{commit_baseline, init_repo, write};

const CONFIG: &str = r#"[gate]
preset = "custom"
strict = true

[budgets.files]
max_bytes = 10

[budgets.functions]
max_lines = 1000
max_cyclomatic = 100
max_cognitive = 100
max_parameters = 20
max_nesting_depth = 20

[clones]
enabled = true
min_lines = 3
min_tokens = 10
"#;

const COPIED: &str = r#"
fn calculate_total(values: &[i32]) -> i32 {
    let mut total = 0;
    for value in values {
        if *value > 0 {
            total += *value;
        }
    }
    total
}
"#;

fn diff_fixture(tag: &str) -> std::path::PathBuf {
    let root = tempdir(tag);
    write(&root, "hardgate.toml", CONFIG);
    write(&root, "src/changed.rs", "pub fn changed() -> i32 { 41 }\n");
    write(&root, "src/scoped/unchanged.rs", COPIED);
    write(&root, "src/unrelated.rs", COPIED);
    init_repo(&root);
    commit_baseline(&root, "baseline");
    write(&root, "src/changed.rs", "pub fn changed() -> i32 { 42 }\n");
    root
}

#[test]
fn diff_scope_adds_explicit_directory_without_hiding_changed_files() {
    let root = diff_fixture("cli-diff-explicit-scope");
    let scope = root.join("src/scoped");
    let scope = format!("{}/", scope.display());
    let output = run(
        &root,
        &["check", "--diff", scope.as_str(), "--format", "json"],
    );
    assert!(
        !output.status.success(),
        "scope should expose budget/clone debt"
    );
    let report = json(&output);

    assert_eq!(report["files_scanned"], 2);
    let budgets = report["budget_violations"].as_array().unwrap();
    assert!(
        budgets
            .iter()
            .any(|violation| { violation["file"] == "src/changed.rs" })
    );
    assert!(
        budgets
            .iter()
            .any(|violation| { violation["file"] == "src/scoped/unchanged.rs" })
    );
    assert!(
        !budgets
            .iter()
            .any(|violation| { violation["file"] == "src/unrelated.rs" })
    );

    let clones = report["clone_violations"].as_array().unwrap();
    assert!(clones.iter().any(|violation| {
        let a = violation["file_a"].as_str().unwrap();
        let b = violation["file_b"].as_str().unwrap();
        [a, b].contains(&"src/scoped/unchanged.rs") && [a, b].contains(&"src/unrelated.rs")
    }));
}

#[test]
fn diff_scope_repository_root_includes_changed_and_unchanged_files() {
    let root = diff_fixture("cli-diff-explicit-root-scope");
    let absolute = root.to_str().expect("fixture path should be UTF-8");

    for scope in [absolute, "."] {
        let output = run(&root, &["check", "--diff", scope, "--format", "json"]);
        assert!(!output.status.success());
        let report = json(&output);
        assert_eq!(report["files_scanned"], 4);
        for file in [
            "src/changed.rs",
            "src/scoped/unchanged.rs",
            "src/unrelated.rs",
        ] {
            assert!(
                report["budget_violations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|violation| violation["file"] == file),
                "{scope} should select {file}"
            );
        }
    }
}
