use hardgate::commands::run_static_gate_snapshot;
use hardgate::config::HardgateConfig;
use hardgate::engines::check_content_budgets;
use std::path::{Path, PathBuf};

#[test]
fn content_budgets_use_supplied_bytes_instead_of_the_worktree() {
    let mut config = HardgateConfig::default();
    config.budgets.files.max_bytes = Some(4);
    config.budgets.files.max_lines.insert("rs".into(), 1);
    let path = Path::new("src/not-materialized-budget-fixture.rs");

    let violations = check_content_budgets(
        path,
        "fn one() {}\nfn two() {}\n",
        &config.budgets.files,
        Path::new("."),
    );

    assert_eq!(violations.len(), 2);
    assert!(
        violations
            .iter()
            .any(|item| item.metric == "File Byte Size")
    );
    assert!(
        violations
            .iter()
            .any(|item| item.metric == "Physical Lines (.rs)")
    );
}

#[test]
fn static_snapshot_analyzes_git_blob_content_without_materializing_it() {
    let mut config = HardgateConfig::default();
    config.clones.enabled = false;
    config.budgets.files.max_lines.insert("rs".into(), 1);
    config.budgets.functions.max_cyclomatic = Some(0);
    let path = PathBuf::from("src/not-materialized-snapshot-fixture.rs");
    let content = "fn decide(value: bool) -> bool {\n    if value { true } else { false }\n}\n";

    let (report, files, loaded, functions) =
        run_static_gate_snapshot(&config, &[(path.clone(), content.into())]).unwrap();

    assert_eq!(files, vec![path.clone()]);
    assert_eq!(loaded, vec![(path.clone(), content.into())]);
    assert_eq!(report.budget_violations.len(), 1);
    assert!(!report.complexity_violations.is_empty());
    assert!(!functions.is_empty());
    assert!(
        !path.exists(),
        "snapshot analysis must not create worktree files"
    );
}
