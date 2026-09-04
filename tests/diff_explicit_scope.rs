#[path = "common/cli.rs"]
mod cli;
#[path = "support/fs.rs"]
mod fs;
#[path = "common/fs_git.rs"]
mod fs_git;

use cli::{json, run};
use fs::tempdir;
use fs_git::{commit_baseline, init_repo, write};

#[test]
fn diff_scope_adds_explicit_directory_without_hiding_changed_files() {
    let root = tempdir("cli-diff-explicit-scope");
    write(
        &root,
        "hardgate.toml",
        r#"[gate]
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
"#,
    );
    write(&root, "src/changed.rs", "pub fn changed() -> i32 { 41 }\n");
    let copied = r#"
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
    write(&root, "src/scoped/unchanged.rs", copied);
    write(&root, "src/unrelated.rs", copied);
    init_repo(&root);
    commit_baseline(&root, "baseline");

    write(&root, "src/changed.rs", "pub fn changed() -> i32 { 42 }\n");
    let scope = root.join("src/scoped");
    let scope = scope.to_str().expect("fixture path should be UTF-8");
    let output = run(&root, &["check", "--diff", scope, "--format", "json"]);
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
