#[path = "support/fs.rs"]
mod fs;

use fs::tempdir;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BASE_CONFIG: &str = r#"[gate]
preset = "custom"
strict = true
enforce_classified_sources = true

[budgets.files]
max_bytes = 100000

[budgets.files.max_lines]
default = 10000
rs = 10000

[budgets.functions]
max_cyclomatic = 100
max_cognitive = 100
max_parameters = 20
max_lines = 1000
max_nesting_depth = 20

[anti_gaming]
disallow_suppressions = true

[coverage]
enabled = false

[mutation]
enabled = false
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, config: &str, source: Option<(&str, &str)>) -> Self {
        let root = tempdir(&format!("cli-json-{tag}"));
        write(&root, "hardgate.toml", config);
        if let Some((path, content)) = source {
            write(&root, path, content);
        }
        Self(root)
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

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    target
        .parent()
        .map(std::fs::create_dir_all)
        .transpose()
        .unwrap();
    std::fs::write(target, content).unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("hardgate binary should run")
}

fn run_scoped_mutation(root: &Path, test_cmd: &str, format: &str) -> Output {
    let args = [
        "mutate",
        "--scoped",
        "src/lib.rs",
        "--test-cmd",
        test_cmd,
        "--max-mutants",
        "1",
        format,
    ];
    run(root, &args)
}

fn parse_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout must contain exactly one JSON document: {error}: {stdout}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn assert_baseline_failure(output: &Output, kind: &str, message: &str) {
    assert!(!output.status.success());
    let failure = parse_stdout(output);
    assert_eq!(failure["passed"], false);
    assert_eq!(failure["stage"], "baseline");
    assert_eq!(failure["kind"], kind);
    assert!(failure["message"].as_str().unwrap().contains(message));
}

fn assert_noop(output: &Output, stage: &str, kind: &str) -> Value {
    assert!(output.status.success());
    let noop = parse_stdout(output);
    assert_eq!(noop["passed"], true);
    assert_eq!(noop["status"], "noop");
    assert_eq!(noop["stage"], stage);
    assert_eq!(noop["kind"], kind);
    noop
}

fn assert_execution_failure(output: &Output, kind: &str) {
    assert!(!output.status.success());
    let failure = parse_stdout(output);
    assert_eq!(failure["passed"], false);
    assert_eq!(failure["stage"], "execution");
    assert_eq!(failure["kind"], kind);
    assert!(failure["message"].as_str().unwrap().contains("mutant"));
}

fn mutation_config(min_score: f64) -> String {
    BASE_CONFIG.replace(
        "[mutation]\nenabled = false",
        &format!("[mutation]\nenabled = true\nmin_score = {min_score}\ntimeout_secs = 2"),
    )
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn init_git(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "hardgate@example.invalid"]);
    git(root, &["config", "user.name", "Hardgate JSON Test"]);
    git(root, &["config", "commit.gpgsign", "false"]);
}

#[test]
fn check_verify_and_scan_json_have_no_progress_prefix_or_suffix() {
    let fixture = Fixture::new(
        "static",
        BASE_CONFIG,
        Some(("src/lib.rs", "pub fn answer() -> i32 { 42 }\n")),
    );
    for args in [
        &["check", "--json"][..],
        &["verify", "--format", "json"][..],
        &["scan", "src/lib.rs", "--format", "json"][..],
    ] {
        let output = run(fixture.as_ref(), args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = parse_stdout(&output);
        assert_eq!(report["passed"], true);
    }
}

#[test]
fn mutate_json_success_keeps_stats_and_results_schema() {
    let fixture = Fixture::new(
        "mutate-success",
        &mutation_config(0.0),
        Some((
            "src/lib.rs",
            "pub fn accepts(value: bool) -> bool { value == true }\n",
        )),
    );
    let output = run_scoped_mutation(
        fixture.as_ref(),
        "sh -c 'grep -q \"== true\" src/lib.rs'",
        "--format=json",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_stdout(&output);
    assert_eq!(report["passed"], true);
    assert!(report["stats"].is_object());
    assert!(report["results"].is_array());
    assert_eq!(report["stats"]["total"], 1);
}

#[test]
fn mutate_json_below_score_is_one_report_and_nonzero() {
    let fixture = Fixture::new(
        "mutate-score",
        &mutation_config(85.0),
        Some((
            "src/lib.rs",
            "pub fn accepts(value: bool) -> bool { value == true }\n",
        )),
    );
    let output = run_scoped_mutation(fixture.as_ref(), "sh -c 'exit 0'", "--format=json");
    assert!(!output.status.success());
    let report = parse_stdout(&output);
    assert_eq!(report["passed"], false);
    assert!(report["stats"].is_object());
    assert!(report["results"].is_array());
}

#[test]
fn mutate_json_baseline_failures_are_typed_and_silent() {
    let fixture = Fixture::new(
        "mutate-baseline-failure",
        &mutation_config(0.0),
        Some((
            "src/lib.rs",
            "pub fn accepts(value: bool) -> bool { value == true }\n",
        )),
    );
    let output = run_scoped_mutation(fixture.as_ref(), "sh -c 'exit 1'", "--format=json");
    assert_baseline_failure(&output, "test-failure", "unmutated baseline");
}

#[test]
fn mutate_json_missing_command_is_a_runner_error_document() {
    let fixture = Fixture::new(
        "mutate-missing-command",
        &mutation_config(0.0),
        Some((
            "src/lib.rs",
            "pub fn accepts(value: bool) -> bool { value == true }\n",
        )),
    );
    let output = run_scoped_mutation(
        fixture.as_ref(),
        "hardgate-command-that-does-not-exist",
        "--json",
    );
    assert_baseline_failure(&output, "runner-error", "Failed to execute");
}

#[test]
fn mutate_json_execution_timeout_is_typed_and_silent() {
    let fixture = Fixture::new(
        "mutate-execution-timeout",
        &mutation_config(0.0),
        Some((
            "src/lib.rs",
            "pub fn accepts(value: bool) -> bool { value == true }\n",
        )),
    );
    let output = run(
        fixture.as_ref(),
        &[
            "mutate",
            "--scoped",
            "src/lib.rs",
            "--test-cmd",
            "sh -c 'grep -q \"== true\" src/lib.rs || sleep 5'",
            "--timeout",
            "1",
            "--max-mutants",
            "1",
            "--format",
            "json",
        ],
    );
    assert_execution_failure(&output, "timeout");
}

#[test]
fn mutate_json_restore_failure_is_an_execution_error_document() {
    let fixture = Fixture::new(
        "mutate-restore-failure",
        &mutation_config(0.0),
        Some((
            "src/lib.rs",
            "pub fn accepts(value: bool) -> bool { value == true }\n",
        )),
    );
    let output = run_scoped_mutation(
        fixture.as_ref(),
        "sh -c 'grep -q \"== true\" src/lib.rs || rm -rf src'",
        "--format=json",
    );
    assert_execution_failure(&output, "execution-error");
}

#[test]
fn mutate_json_disabled_policy_is_an_explicit_success_noop() {
    let fixture = Fixture::new(
        "mutate-disabled",
        BASE_CONFIG,
        Some(("src/lib.rs", "pub fn answer() -> i32 { 42 }\n")),
    );
    let output = run(fixture.as_ref(), &["mutate", "--json"]);
    let noop = assert_noop(&output, "policy", "disabled");
    assert!(noop["message"].as_str().unwrap().contains("disabled"));
}

#[test]
fn mutate_json_diff_without_changed_targets_is_an_explicit_success_noop() {
    let fixture = Fixture::new(
        "mutate-diff-noop",
        &mutation_config(85.0),
        Some(("src/lib.rs", "pub fn answer() -> i32 { 42 }\n")),
    );
    init_git(fixture.as_ref());
    git(fixture.as_ref(), &["add", "-A"]);
    git(fixture.as_ref(), &["commit", "-qm", "baseline"]);

    let output = run(fixture.as_ref(), &["mutate", "--diff", "--json"]);
    assert_noop(&output, "selection", "no-changed-targets");
}
