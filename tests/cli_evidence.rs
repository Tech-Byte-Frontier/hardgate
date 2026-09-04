#[path = "support/fs.rs"]
mod fs;
#[path = "common/fs_git.rs"]
mod fs_git;

use fs::tempdir;
use fs_git::{commit_baseline, init_repo, write};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("hardgate binary should run")
}

fn json(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}: {stdout}; stderr: {stderr}"))
}

fn successful_report(root: &Path, command: &str) -> Value {
    let output = run(root, &[command, "--format", "json"]);
    assert!(
        output.status.success(),
        "{command}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    json(&output)
}

fn failed_report(root: &Path, command: &str) -> Value {
    let output = run(root, &[command, "--format", "json"]);
    assert!(!output.status.success(), "{command} unexpectedly passed");
    json(&output)
}

fn base_config(extra: &str) -> String {
    format!(
        r#"[gate]
preset = "custom"
strict = true

[budgets.files]
max_bytes = 100000

[budgets.functions]
max_lines = 1000
max_cyclomatic = 100
max_cognitive = 100
max_parameters = 20
max_nesting_depth = 20

{extra}
"#
    )
}

fn diff_fixture(
    tag: &str,
    baseline_source: &str,
    current_source: &str,
    coverage: &str,
) -> std::path::PathBuf {
    let root = tempdir(tag);
    write(
        &root,
        "hardgate.toml",
        &base_config(
            r#"[coverage]
enabled = true
report = "coverage.info"
min_line_percent = 90.0
"#,
        ),
    );
    write(&root, "src/lib.rs", baseline_source);
    init_repo(&root);
    commit_baseline(&root, "baseline");
    write(&root, "src/lib.rs", current_source);
    write(&root, "coverage.info", coverage);
    root
}

fn failed_diff_report(root: &Path) -> Value {
    let output = run(root, &["check", "--diff", "--format", "json"]);
    assert!(!output.status.success());
    json(&output)
}

fn malformed_legacy_fixture(tag: &str, strict: bool) -> std::path::PathBuf {
    let mut config = base_config(
        r#"[legacy]
reference_branch = "HEAD"
ratchet = true
"#,
    );
    config = config.replace("max_bytes = 100000", "max_bytes = 1");
    if !strict {
        config = config.replace("strict = true", "strict = false");
    }
    let root = tempdir(tag);
    write(&root, "hardgate.toml", &config);
    write(&root, "src/lib.rs", "pub fn broken( -> i32 { 1 }\n");
    init_repo(&root);
    commit_baseline(&root, "malformed baseline");
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    root
}

fn malformed_legacy_reports(root: &Path) -> [Value; 2] {
    ["check", "verify"].map(|command| failed_report(root, command))
}

fn assert_malformed_legacy_report(report: &Value) {
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|violation| violation["step"] == "legacy-ratchet"),
        "invalid baseline evidence must block the ratchet"
    );
    assert!(
        report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|advisory| advisory.as_str().unwrap().contains("grandfathered=0")),
        "an untrusted advisory baseline must not grandfather debt"
    );
}

#[test]
fn generated_freshness_runs_for_check_and_verify_and_reports_success() {
    let root = tempdir("cli-generated");
    let config = base_config(
        r#"[generated]
enabled = true
freshness_command = "printf generated-ok"
"#,
    );
    write(&root, "hardgate.toml", &config);
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    for command in ["check", "verify"] {
        let report = successful_report(&root, command);
        assert!(
            report["orchestration_violations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            report["advisories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|advisory| advisory
                    .as_str()
                    .unwrap()
                    .contains("generated-freshness evidence"))
        );
    }
}

#[test]
fn generated_freshness_failure_fails_check_and_verify() {
    let root = tempdir("cli-generated-failure");
    let config = base_config(
        r#"[generated]
enabled = true
freshness_command = "sh -c 'exit 7'"
"#,
    );
    write(&root, "hardgate.toml", &config);
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    for command in ["check", "verify"] {
        let report = failed_report(&root, command);
        assert!(
            report["orchestration_violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|violation| violation["step"] == "generated-freshness")
        );
    }
}

#[test]
fn configured_dead_code_runs_for_check_and_verify() {
    let root = tempdir("cli-dead-code");
    let config = base_config(
        r#"[analysis.dead_code]
enabled = true
"#,
    );
    write(&root, "hardgate.toml", &config);
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    write(&root, "src/unused.rs", "pub fn old_code() -> i32 { 1 }\n");

    for command in ["check", "verify"] {
        let report = failed_report(&root, command);
        assert!(
            report["dead_code_violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|violation| violation["file"] == "src/unused.rs")
        );
    }
}

#[test]
fn check_diff_reports_uncovered_and_missing_changed_source_lines() {
    let root = diff_fixture(
        "cli-diff-coverage",
        "pub fn answer() -> i32 { 42 }\n",
        "pub fn answer() -> i32 { 43 }\n",
        "SF:src/lib.rs\nDA:1,0\nLF:1\nLH:0\nend_of_record\n",
    );
    write(&root, "src/new.rs", "pub fn new_value() -> i32 { 7 }\n");

    let report = failed_diff_report(&root);
    let violations = report["coverage_violations"].as_array().unwrap();
    assert!(
        violations
            .iter()
            .any(|violation| violation["metric"] == "Diff Line Coverage")
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation["metric"] == "Missing Diff Coverage")
    );
}

#[test]
fn check_diff_requires_da_for_changed_code_but_ignores_comments_and_braces() {
    let root = diff_fixture(
        "cli-diff-missing-da",
        "pub fn answer() -> i32 {\n    41\n}\n",
        "pub fn answer() -> i32 {\n    // changed comment\n    let value = 42;\n    value\n}\n",
        "SF:src/lib.rs\nDA:1,1\nDA:4,1\nLF:2\nLH:2\nend_of_record\n",
    );

    let report = failed_diff_report(&root);
    let violations = report["coverage_violations"].as_array().unwrap();
    assert!(violations.iter().any(|violation| {
        violation["metric"] == "Missing Diff Coverage"
            && violation["message"].as_str().unwrap().contains("3")
    }));
    assert!(!violations.iter().any(|violation| {
        violation["metric"] == "Missing Diff Coverage"
            && violation["message"].as_str().unwrap().contains("2")
    }));
}

#[test]
fn verify_coverage_scores_only_current_source_role_records() {
    let root = tempdir("cli-source-only-coverage");
    let config = base_config(
        r#"[coverage]
enabled = true
report = "coverage.info"
min_line_percent = 90.0
"#,
    );
    write(&root, "hardgate.toml", &config);
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    write(
        &root,
        "tests/lib.rs",
        "pub fn test_helper() -> i32 { 42 }\n",
    );
    write(
        &root,
        "coverage.info",
        "SF:src/lib.rs\nDA:1,0\nLF:1\nLH:0\nend_of_record\nSF:tests/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    );

    let report = failed_report(&root, "verify");
    let global = report["coverage_violations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|violation| violation["metric"] == "Global Line Coverage")
        .expect("source floor should be evaluated");
    assert_eq!(global["actual"], 0.0);
}

#[test]
fn malformed_coverage_report_is_blocking_orchestration_evidence() {
    let root = tempdir("cli-malformed-coverage");
    let config = base_config(
        r#"[coverage]
enabled = true
report = "coverage.info"
min_line_percent = 90.0
"#,
    );
    write(&root, "hardgate.toml", &config);
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    write(
        &root,
        "coverage.info",
        "SF:src/lib.rs\nDA:1,wat\nend_of_record\n",
    );

    let report = failed_report(&root, "verify");
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|violation| violation["step"] == "coverage-report")
    );
}

#[test]
fn required_coverage_report_cannot_be_skipped_when_no_source_exists() {
    let root = tempdir("cli-empty-required-report");
    write(
        &root,
        "hardgate.toml",
        &base_config(
            r#"[coverage]
enabled = true
report = "missing.info"
"#,
        ),
    );

    let output = run(&root, &["verify", "--format", "json"]);
    assert!(!output.status.success());
    let report = json(&output);
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|violation| violation["step"] == "coverage-report")
    );
}

#[test]
fn non_strict_enabled_evidence_still_fails_closed() {
    let root = tempdir("cli-nonstrict-evidence");
    let mut config = base_config("");
    config = config.replace("strict = true", "strict = false");
    config.push_str(
        r#"[coverage]
enabled = true
report = "missing.info"

[mutation]
enabled = true
reports = ["missing.json"]
"#,
    );
    write(&root, "hardgate.toml", &config);
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    let output = run(&root, &["verify", "--format", "json"]);
    assert!(!output.status.success());
    let report = json(&output);
    let failures = report["orchestration_violations"].as_array().unwrap();
    assert!(
        failures
            .iter()
            .any(|violation| violation["step"] == "coverage-report")
    );
    assert!(
        failures
            .iter()
            .any(|violation| violation["step"] == "mutation-report")
    );
}

#[test]
fn legacy_ratchet_grandfathers_current_dead_code_and_emits_summary() {
    let root = tempdir("cli-legacy-ratchet");
    let config = base_config(
        r#"[analysis.dead_code]
enabled = true

[legacy]
reference_branch = "HEAD"
ratchet = true
"#,
    );
    write(&root, "hardgate.toml", &config);
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    write(&root, "src/unused.rs", "pub fn old_code() -> i32 { 1 }\n");
    init_repo(&root);
    commit_baseline(&root, "baseline");

    let report = successful_report(&root, "check");
    assert!(
        report["dead_code_violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|advisory| {
                let text = advisory.as_str().unwrap();
                text.contains("legacy ratchet")
                    && text.contains("reference")
                    && text.contains("merge-base")
                    && text.contains("grandfathered=1")
            })
    );
}

#[test]
fn malformed_legacy_baseline_blocks_check_and_verify_without_grandfathering_debt() {
    let root = malformed_legacy_fixture("cli-legacy-malformed-baseline", true);

    for report in malformed_legacy_reports(&root) {
        assert!(
            report["budget_violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|violation| violation["file"] == "src/lib.rs"),
            "current static debt must remain when the baseline is malformed"
        );
        assert_malformed_legacy_report(&report);
    }
}

#[test]
fn malformed_legacy_baseline_is_blocking_even_when_current_roles_are_advisory() {
    let root = malformed_legacy_fixture("cli-legacy-advisory-malformed-baseline", false);

    for report in malformed_legacy_reports(&root) {
        assert_malformed_legacy_report(&report);
    }
}
