#[path = "common/cli.rs"]
mod cli;
#[path = "common/fs_git.rs"]
mod fs_git;

use cli::{Fixture, assert_status, json, run};
use fs_git::{commit_baseline, init_repo, write};
use serde_json::Value;
use std::path::Path;

fn successful_report(root: &Path, command: &str) -> Value {
    let output = run(root, &[command, "--checks", "policy", "--format", "json"]);
    assert_status(&output, true, command);
    json(&output)
}

fn failed_report(root: &Path, command: &str) -> Value {
    let output = run(root, &[command, "--checks", "policy", "--format", "json"]);
    assert_status(&output, false, command);
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
max_parameters = 20
max_nesting_depth = 20

{extra}
"#
    )
}

fn malformed_legacy_fixture(tag: &str, strict: bool) -> Fixture {
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
    let root = Fixture::new("cli-evidence", tag, Some(&config));
    write(&root, "src/lib.rs", "pub fn broken( -> i32 { 1 }\n");
    init_repo(&root);
    commit_baseline(&root, "malformed baseline");
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    root
}

fn malformed_legacy_reports(root: &Path) -> [Value; 1] {
    ["check"].map(|command| failed_report(root, command))
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
fn generated_freshness_runs_for_policy_check_and_reports_success() {
    let config = base_config(
        r#"[generated]
enabled = true
freshness_command = "printf generated-ok"
"#,
    );
    let root = Fixture::new("cli-evidence", "generated", Some(&config));
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    let command = "check";
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

#[test]
fn generated_freshness_failure_fails_policy_check() {
    let config = base_config(
        r#"[generated]
enabled = true
freshness_command = "sh -c 'exit 7'"
"#,
    );
    let root = Fixture::new("cli-evidence", "generated-failure", Some(&config));
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    let command = "check";
    let report = failed_report(&root, command);
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|violation| violation["step"] == "generated-freshness")
    );
}

#[test]
fn unbound_coverage_report_is_blocking_orchestration_evidence() {
    let config = base_config(
        r#"[coverage]
enabled = true
report = "coverage.info"
min_line_percent = 90.0
"#,
    );
    let root = Fixture::new("cli-evidence", "malformed-coverage", Some(&config));
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    write(
        &root,
        "coverage.info",
        "SF:src/lib.rs\nDA:1,wat\nend_of_record\n",
    );

    let report = failed_report(&root, "check");
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|violation| violation["step"] == "coverage-report")
    );
}

#[test]
fn coverage_report_is_not_auto_discovered_for_policy_check() {
    let config = base_config(
        r#"[coverage]
enabled = true
min_line_percent = 90.0
"#,
    );
    let root = Fixture::new("cli-evidence", "coverage-no-auto-discovery", Some(&config));
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    write(
        &root,
        "coverage/lcov.info",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    );

    let args = ["check", "--checks", "policy", "--format", "json"];
    let output = run(&root, &args);
    assert!(
        !output.status.success(),
        "coverage should require an explicit CLI/config path for {args:?}"
    );
    let report = json(&output);
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|violation| {
                violation["step"] == "coverage-report" && violation["command"] == "<not-configured>"
            }),
        "missing configured report must block {args:?}: {report}"
    );
}

#[test]
fn required_coverage_report_cannot_be_skipped_when_no_source_exists() {
    let config = base_config(
        r#"[coverage]
enabled = true
report = "missing.info"
"#,
    );
    let root = Fixture::new("cli-evidence", "empty-required-report", Some(&config));

    let output = run(&root, &["check", "--checks", "policy", "--format", "json"]);
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
    let root = Fixture::new("cli-evidence", "nonstrict-evidence", Some(&config));
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    let output = run(&root, &["check", "--checks", "policy", "--format", "json"]);
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
fn malformed_legacy_baseline_blocks_policy_check_without_grandfathering_debt() {
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
