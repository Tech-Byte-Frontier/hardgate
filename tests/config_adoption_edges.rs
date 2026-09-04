use hardgate::GateReport;
use hardgate::adoption::ratchet_report;
use hardgate::config::{
    ExclusionConfig, FileBudgets, GeneratedConfig, HardgateConfig, LegacyConfig, RolePoliciesConfig,
};
use hardgate::engines::{
    BudgetViolation, CloneViolation, CoverageViolation, SuppressionViolation,
    check_content_budgets, check_file_budgets,
};
use hardgate::git_evidence::{ChangeSet, ChangedLineMap};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn write_config(tag: &str, body: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "hardgate-config-adoption-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hardgate.toml");
    std::fs::write(&path, body).unwrap();
    (dir, path)
}

#[test]
fn role_generated_and_legacy_validation_rejects_unsafe_edges() {
    let mut roles = RolePoliciesConfig::default();
    roles.source.max_abc = Some(1.5);
    roles.source.max_halstead_difficulty = Some(2.0);
    assert!(roles.validate().is_ok());

    roles.source.max_abc = Some(0.0);
    assert!(roles.validate().is_err());
    roles.source.max_abc = Some(f64::NAN);
    assert!(roles.validate().is_err());

    let empty_command = GeneratedConfig {
        enabled: false,
        freshness_command: Some("   ".to_string()),
        timeout_secs: Some(1),
    };
    assert!(empty_command.validate().is_err());
    let zero_timeout = GeneratedConfig {
        enabled: false,
        freshness_command: None,
        timeout_secs: Some(0),
    };
    assert!(zero_timeout.validate().is_err());
    let inherited_timeout = GeneratedConfig {
        enabled: false,
        freshness_command: None,
        timeout_secs: None,
    };
    assert!(inherited_timeout.validate().is_ok());

    let missing_branch = LegacyConfig {
        reference_branch: None,
        ratchet: true,
    };
    assert!(missing_branch.validate().is_err());
    let empty_branch = LegacyConfig {
        reference_branch: Some(" ".to_string()),
        ratchet: false,
    };
    assert!(empty_branch.validate().is_err());
}

#[test]
fn config_validation_covers_nested_globs_and_numeric_rejections() {
    let (dir, path) = write_config("empty-gate", "[gate]\npreset = \"custom\"\nname = \" \"\n");
    assert!(HardgateConfig::load_or_default(Some(&path)).is_err());
    let _ = std::fs::remove_dir_all(&dir);

    let (dir, path) = write_config(
        "bad-percent",
        "[gate]\npreset = \"custom\"\n\n[coverage]\nmin_line_percent = 101.0\n",
    );
    assert!(HardgateConfig::load_or_default(Some(&path)).is_err());
    let _ = std::fs::remove_dir_all(&dir);

    let (dir, path) = write_config(
        "nan-percent",
        "[gate]\npreset = \"custom\"\n\n[coverage]\nmin_line_percent = nan\n",
    );
    assert!(HardgateConfig::load_or_default(Some(&path)).is_err());
    let _ = std::fs::remove_dir_all(&dir);

    let (dir, path) = write_config(
        "bad-crap",
        "[gate]\npreset = \"custom\"\n\n[coverage]\nmax_crap_score = -1.0\n",
    );
    assert!(HardgateConfig::load_or_default(Some(&path)).is_err());
    let _ = std::fs::remove_dir_all(&dir);

    let (dir, path) = write_config(
        "nan-crap",
        "[gate]\npreset = \"custom\"\n\n[coverage]\nmax_crap_score = nan\n",
    );
    assert!(HardgateConfig::load_or_default(Some(&path)).is_err());
    let _ = std::fs::remove_dir_all(&dir);

    let (dir, path) = write_config(
        "nested-globs",
        r#"
[gate]
preset = "custom"

[budgets.files.exclusions]
paths = ["generated/**"]

[analysis.dead_code]
exclude = ["vendor/**"]
entry_points = ["src/main.rs"]

[[invariants.rules]]
from = "src/**"
exclude = ["src/generated/**"]
disallow_imports = ["private/**"]
"#,
    );
    assert!(HardgateConfig::load_or_default(Some(&path)).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_alias_and_empty_tables_follow_presence_semantics() {
    let (dir, path) = write_config(
        "merge-presence",
        r#"
[gate]
preset = "balanced"

[role_policies.source]
max_lines = 77

[budgets.files]
max_lines = {}

[budgets.files.exclusions]
"#,
    );
    let config = HardgateConfig::load_or_default(Some(&path)).unwrap();
    assert_eq!(config.roles.source.max_lines, Some(77));
    assert!(config.budgets.files.max_lines.is_empty());
    assert!(config.budgets.files.exclusions.paths.is_empty());
    let _ = std::fs::remove_dir_all(&dir);

    let (dir, path) = write_config(
        "merge-absent-tables",
        r#"
[gate]
preset = "balanced"

[budgets.files]
max_bytes = 123
"#,
    );
    let config = HardgateConfig::load_or_default(Some(&path)).unwrap();
    assert_eq!(config.budgets.files.max_bytes, Some(123));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn budget_edges_handle_missing_files_exact_exclusions_and_bad_globs() {
    let missing = Path::new("/tmp/hardgate-wave2-no-such-file.rs");
    let empty = FileBudgets::default();
    assert!(check_file_budgets(missing, &empty, Path::new("/tmp")).is_empty());

    let invalid_pattern = FileBudgets {
        max_bytes: Some(2),
        max_lines: HashMap::new(),
        exclusions: ExclusionConfig {
            paths: vec!["[invalid".to_string()],
        },
    };
    let violations = check_content_budgets(
        Path::new("src/main.rs"),
        "four",
        &invalid_pattern,
        Path::new("."),
    );
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].metric, "File Byte Size");

    let exact = FileBudgets {
        max_bytes: Some(2),
        max_lines: HashMap::new(),
        exclusions: ExclusionConfig {
            paths: vec!["src/main.rs".to_string()],
        },
    };
    assert!(
        check_content_budgets(Path::new("src/main.rs"), "four", &exact, Path::new(".")).is_empty()
    );
}

fn changes(
    changed_lines: &[(&str, &[usize])],
    changed_files: &[&str],
    renames: &[(&str, &str)],
) -> ChangeSet {
    let mut lines = ChangedLineMap::new();
    for (path, numbers) in changed_lines {
        lines.insert(PathBuf::from(path), numbers.iter().copied().collect());
    }
    ChangeSet {
        merge_base: "wave2-merge-base".to_string(),
        changed_lines: lines,
        changed_files: changed_files.iter().map(PathBuf::from).collect(),
        rename_lineage: renames
            .iter()
            .map(|(current, baseline)| (PathBuf::from(current), PathBuf::from(baseline)))
            .collect(),
    }
}

fn budget(file: &str, actual: usize, message: &str) -> BudgetViolation {
    BudgetViolation {
        file: file.into(),
        metric: "lines".to_string(),
        actual,
        limit: 10,
        message: message.to_string(),
    }
}

fn suppression(file: &str) -> SuppressionViolation {
    SuppressionViolation {
        file: file.into(),
        line_number: 2,
        token: "@ts-ignore".to_string(),
        line_content: "// @ts-ignore".to_string(),
        message: "suppression debt".to_string(),
    }
}

fn clone_violation() -> CloneViolation {
    CloneViolation {
        file_a: "src/a.rs".into(),
        lines_a: (1, 3),
        file_b: "src/b.rs".into(),
        lines_b: (2, 4),
        tokens: 50,
        lines: 3,
        fingerprint: "fingerprint".to_string(),
        message: "clone debt".to_string(),
        recommendation: "extract".to_string(),
    }
}

#[test]
fn adoption_wrapper_skips_existing_context_and_handles_empty_messages() {
    let mut current = GateReport::new("legacy".to_string());
    current
        .budget_violations
        .push(budget("src/a.rs", 100, "budget debt [changed file prior]"));
    current.coverage_violations.push(CoverageViolation {
        file: "src/a.rs".into(),
        function_name: None,
        metric: "line coverage".to_string(),
        actual: 50.0,
        limit: 90.0,
        message: String::new(),
        recommendation: "cover".to_string(),
    });
    let change_set = changes(&[("src/a.rs", &[])], &["src/a.rs"], &[]);
    let outcome = ratchet_report(
        &mut current,
        &GateReport::new("legacy".to_string()),
        &change_set,
    );

    assert_eq!(outcome.retained, 2);
    assert_eq!(
        current.budget_violations[0].message,
        "budget debt [changed file prior]"
    );
    assert!(
        current.coverage_violations[0]
            .message
            .contains("[changed file `src/a.rs`]")
    );
}

#[test]
fn adoption_clone_context_formats_sparse_ranges_and_rename_cycles() {
    let mut baseline = GateReport::new("legacy".to_string());
    baseline
        .budget_violations
        .push(budget("src/old.rs", 100, "budget debt"));
    let mut current = GateReport::new("legacy".to_string());
    current
        .budget_violations
        .push(budget("src/new.rs", 100, "budget debt"));
    current.clone_violations.push(clone_violation());

    let change_set = changes(
        &[("src/a.rs", &[1, 3])],
        &["src/a.rs", "src/b.rs"],
        &[("src/new.rs", "src/old.rs"), ("src/old.rs", "src/new.rs")],
    );
    let outcome = ratchet_report(&mut current, &baseline, &change_set);

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(current.budget_violations.len(), 1);
    assert!(
        current.clone_violations[0]
            .message
            .contains("`src/a.rs`:1,3")
    );
    assert!(current.clone_violations[0].message.contains("`src/b.rs`"));
}

#[test]
fn adoption_multiset_does_not_grandfather_more_than_baseline_count() {
    let baseline_finding = suppression("src/a.rs");
    let mut baseline = GateReport::new("legacy".to_string());
    baseline
        .suppression_violations
        .push(baseline_finding.clone());

    let mut current = GateReport::new("legacy".to_string());
    current
        .suppression_violations
        .extend([baseline_finding.clone(), baseline_finding]);
    let outcome = ratchet_report(&mut current, &baseline, &changes(&[], &[], &[]));

    assert_eq!(outcome.grandfathered, 1);
    assert_eq!(current.suppression_violations.len(), 1);
    assert!(!current.passed);
}

#[test]
fn adoption_numeric_ratchet_keeps_worsened_untouched_debt() {
    let mut baseline = GateReport::new("legacy".to_string());
    baseline
        .budget_violations
        .push(budget("src/a.rs", 100, "budget debt"));
    let mut current = GateReport::new("legacy".to_string());
    current
        .budget_violations
        .push(budget("src/a.rs", 101, "budget debt"));

    let outcome = ratchet_report(&mut current, &baseline, &changes(&[], &[], &[]));
    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(current.budget_violations.len(), 1);
    assert!(!current.passed);
}
