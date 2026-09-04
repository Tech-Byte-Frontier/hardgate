#[path = "common/cli.rs"]
mod cli;
#[path = "support/fs.rs"]
mod fs;
#[path = "common/fs_git.rs"]
mod fs_git;

use cli::{json, run};
use fs::tempdir;
use fs_git::{commit_baseline, init_repo, write};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Output;

const BASE_CONFIG: &str = r#"[gate]
preset = "custom"
strict = true
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        Self(tempdir(&format!("cli-gate-edges-{tag}")))
    }

    fn write(&self, path: &str, content: &str) {
        write(&self.0, path, content);
    }
}

impl AsRef<Path> for Fixture {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stdout={} stderr={}",
        stdout(output),
        stderr(output)
    );
}

fn report_for(root: &Fixture, args: &[&str]) -> Value {
    let output = run(root.as_ref(), args);
    assert_success(&output, &format!("{args:?}"));
    json(&output)
}

fn failed_report_for(root: &Fixture, args: &[&str]) -> Value {
    let output = run(root.as_ref(), args);
    assert!(!output.status.success(), "{args:?} unexpectedly passed");
    json(&output)
}

fn mutation_config(reports: &str) -> String {
    format!("{BASE_CONFIG}\n[mutation]\nenabled = true\nmin_score = 0.0\n{reports}\n")
}

#[test]
fn verify_requires_each_mutation_report_state_and_accepts_valid_json() {
    let missing_path = Fixture::new("mutation-no-path");
    missing_path.write("hardgate.toml", &mutation_config(""));
    let report = failed_report_for(&missing_path, &["verify", "--format", "json"]);
    assert_mutation_failure(&report, "<not-configured>", "no report path");

    let empty_list = Fixture::new("mutation-empty-list");
    empty_list.write("hardgate.toml", &mutation_config("reports = []"));
    let report = failed_report_for(&empty_list, &["verify", "--format", "json"]);
    assert_mutation_failure(&report, "<empty-report-list>", "report list is empty");

    let missing_file = Fixture::new("mutation-missing-file");
    missing_file.write(
        "hardgate.toml",
        &mutation_config("reports = [\"mutation.json\"]"),
    );
    let report = failed_report_for(&missing_file, &["verify", "--format", "json"]);
    assert_mutation_failure(&report, "mutation.json", "not found");

    let malformed = Fixture::new("mutation-malformed");
    malformed.write(
        "hardgate.toml",
        &mutation_config("reports = [\"mutation.json\"]"),
    );
    malformed.write("mutation.json", "{\n");
    let report = failed_report_for(&malformed, &["verify", "--format", "json"]);
    assert_mutation_failure(&report, "mutation.json", "parse required mutation report");

    let valid = Fixture::new("mutation-valid");
    valid.write(
        "hardgate.toml",
        &mutation_config("reports = [\"mutation.json\"]"),
    );
    valid.write("mutation.json", r#"{"killed":1}"#);
    let report = report_for(&valid, &["verify", "--format", "json"]);
    assert_eq!(report["passed"], true);
    assert!(report["mutation_violations"].as_array().unwrap().is_empty());
}

fn assert_mutation_failure(report: &Value, target: &str, message: &str) {
    let failures = report["orchestration_violations"].as_array().unwrap();
    let failure = failures
        .iter()
        .find(|failure| failure["step"] == "mutation-report" && failure["command"] == target)
        .unwrap_or_else(|| panic!("mutation report failure missing for {target}: {report}"));
    assert!(
        failure["output"].as_str().unwrap().contains(message),
        "unexpected mutation failure: {failure}"
    );
}

#[test]
fn check_diff_reports_invalid_coverage_reference_without_ratchet() {
    let fixture = Fixture::new("diff-invalid-reference");
    fixture.write(
        "hardgate.toml",
        &format!(
            "{BASE_CONFIG}\n[coverage]\nenabled = true\nreport = \"coverage.info\"\nmin_line_percent = 0.0\n\n[legacy]\nreference_branch = \"missing-reference\"\nratchet = false\n"
        ),
    );
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 1 }\n");
    init_repo(fixture.as_ref());
    commit_baseline(fixture.as_ref(), "baseline");
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 2 }\n");
    fixture.write(
        "coverage.info",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    );

    let report = failed_report_for(&fixture, &["check", "--diff", "--format", "json"]);
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|failure| failure["step"] == "coverage-diff"
                && failure["output"]
                    .as_str()
                    .unwrap()
                    .contains("missing-reference"))
    );
}

#[test]
fn check_diff_uses_valid_legacy_reference_for_changed_coverage() {
    let fixture = Fixture::new("diff-valid-reference");
    fixture.write(
        "hardgate.toml",
        &format!(
            "{BASE_CONFIG}\n[coverage]\nenabled = true\nreport = \"coverage.info\"\nmin_line_percent = 0.0\n\n[legacy]\nreference_branch = \"HEAD\"\nratchet = true\n"
        ),
    );
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 1 }\n");
    init_repo(fixture.as_ref());
    commit_baseline(fixture.as_ref(), "baseline");
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 2 }\n");
    fixture.write(
        "coverage.info",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    );

    let report = report_for(&fixture, &["check", "--diff", "--format", "json"]);
    assert_eq!(report["passed"], true);
    assert!(report["coverage_violations"].as_array().unwrap().is_empty());
    assert!(
        report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|advisory| {
                advisory
                    .as_str()
                    .unwrap()
                    .contains("legacy ratchet: reference=`HEAD`")
            })
    );

    let summary = run(&fixture, &["check", "--all", "--format", "summary"]);
    assert_success(&summary, "complete evidence summary");
    assert!(stdout(&summary).contains("result: pass"));
}

#[test]
fn invalid_legacy_reference_is_blocking_and_visible_in_verify() {
    let fixture = Fixture::new("legacy-invalid");
    fixture.write(
        "hardgate.toml",
        &format!(
            "{BASE_CONFIG}\n[legacy]\nreference_branch = \"missing-reference\"\nratchet = true\n"
        ),
    );
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    init_repo(fixture.as_ref());
    commit_baseline(fixture.as_ref(), "baseline");

    let report = failed_report_for(&fixture, &["verify", "--format", "json"]);
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|failure| failure["step"] == "legacy-ratchet"
                && failure["output"]
                    .as_str()
                    .unwrap()
                    .contains("missing-reference"))
    );
    assert!(
        report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|advisory| {
                advisory
                    .as_str()
                    .unwrap()
                    .contains("legacy ratchet: reference=`missing-reference`")
            })
    );
}

#[test]
fn check_all_reports_complete_evidence_advisory() {
    let fixture = Fixture::new("complete-evidence");
    fixture.write(
        "hardgate.toml",
        &format!(
            "{BASE_CONFIG}\n[coverage]\nenabled = true\nreport = \"coverage.info\"\n\n[mutation]\nenabled = true\nmin_score = 0.0\nreports = [\"mutation.json\"]\n\n[analysis.dead_code]\nenabled = true\n\n[orchestration]\nformat_check = \"sh -c 'exit 0'\"\nlint = \"sh -c 'exit 0'\"\ntest_cmd = \"sh -c 'exit 0'\"\ntimeout_secs = 2\n"
        ),
    );
    fixture.write(
        "coverage.info",
        "SF:workspace.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    );
    fixture.write("mutation.json", r#"{"killed":1}"#);

    let report = report_for(&fixture, &["check", "--all", "--format", "json"]);
    assert_eq!(report["passed"], true);
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
            .any(|advisory| {
                advisory
                    .as_str()
                    .unwrap()
                    .contains("evaluated every configured report")
            })
    );
}

#[test]
fn warning_source_role_keeps_suppression_invariant_clone_and_dead_code_visible() {
    let fixture = Fixture::new("warning-role-findings");
    fixture.write(
        "hardgate.toml",
        &format!(
            "{BASE_CONFIG}\n[roles.source]\nseverity = \"warning\"\n\n[clones]\nmin_lines = 5\nmin_tokens = 20\n\n[invariants]\nenforce = true\n[[invariants.rules]]\nname = \"source token boundary\"\nfrom = \"src/**\"\ndisallow_tokens = [\"forbidden\"]\n\n[analysis.dead_code]\nenabled = true\n"
        ),
    );
    fixture.write("src/lib.rs", "pub fn active() -> i32 { 1 }\n");
    fixture.write(
        "src/suppress.rs",
        "#[allow(dead_code)]\npub fn suppressed() -> i32 { 1 }\n",
    );
    fixture.write("src/invariant.rs", "pub fn forbidden() -> i32 { 2 }\n");
    let duplicate = "pub fn repeated(value: i32) -> i32 {\n    let first = value + 1;\n    let second = first * 2;\n    let third = second - 3;\n    let fourth = third / 2;\n    fourth + value\n}\n";
    fixture.write("src/clone_a.rs", duplicate);
    fixture.write("src/clone_b.rs", duplicate);

    let report = report_for(&fixture, &["check", "--format", "json"]);
    assert_eq!(report["passed"], true);
    let advisories = report["advisories"].as_array().unwrap();
    for category in ["suppression", "invariant", "clone", "dead code"] {
        assert!(
            advisories
                .iter()
                .any(|advisory| advisory.as_str().unwrap().contains(category)),
            "missing warning advisory category {category}: {advisories:?}"
        );
    }
    assert!(
        report["suppression_violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        report["invariant_violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(report["clone_violations"].as_array().unwrap().is_empty());
    assert!(
        report["dead_code_violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn empty_check_scopes_report_discovery_advisories_for_diff_and_paths() {
    let diff = Fixture::new("empty-diff");
    diff.write("hardgate.toml", BASE_CONFIG);
    diff.write("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    init_repo(diff.as_ref());
    commit_baseline(diff.as_ref(), "baseline");
    let diff_report = report_for(&diff, &["check", "--diff", "--format", "json"]);
    assert!(
        diff_report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|advisory| {
                advisory
                    .as_str()
                    .unwrap()
                    .contains("no git-modified source files detected")
            })
    );

    let scoped = Fixture::new("empty-scoped");
    scoped.write("hardgate.toml", BASE_CONFIG);
    std::fs::create_dir_all(scoped.0.join("empty")).unwrap();
    let scoped_report = report_for(&scoped, &["check", "empty", "--format", "json"]);
    assert!(
        scoped_report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|advisory| {
                advisory
                    .as_str()
                    .unwrap()
                    .contains("no matching source files detected for the given path(s)")
            })
    );
}
