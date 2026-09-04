#[path = "support/fs.rs"]
mod fs;

use fs::tempdir;
use hardgate::GateReport;
use hardgate::commands::{
    AnalyzeInput, OutputOptions, analyze_file_content, output_report, output_report_with_opts,
    print_empty_discovery,
};
use hardgate::config::{ClassificationRule, HardgateConfig};
use hardgate::discovery::FileRole;
use hardgate::engines::{AntiGamingScanner, InvariantsChecker};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BASE_CONFIG: &str = r#"[gate]
name = "entrypoint-fixture"
preset = "custom"
strict = true

[budgets.files]
max_bytes = 100000

[budgets.functions]
max_cyclomatic = 100
max_cognitive = 100
max_parameters = 20
max_lines = 1000
max_nesting_depth = 20

[clones]
enabled = false

[coverage]
enabled = false

[mutation]
enabled = false
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, config: &str) -> Self {
        let root = tempdir(&format!("entrypoint-{tag}"));
        std::fs::write(root.join("hardgate.toml"), config).unwrap();
        Self(root)
    }

    fn write(&self, path: &str, content: &str) {
        let target = self.0.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, content).unwrap();
    }
}

impl AsRef<Path> for Fixture {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("hardgate binary should run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout should be exactly one JSON document: {error}; stdout={}; stderr={}",
            stdout(output),
            stderr(output)
        )
    })
}

#[test]
fn check_json_flags_have_deterministic_precedence() {
    let fixture = Fixture::new("json-precedence", BASE_CONFIG);
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    let full = run(
        fixture.as_ref(),
        &["check", "--json", "--format", "terminal"],
    );
    assert!(full.status.success(), "{}", stderr(&full));
    let full_report = json(&full);
    assert_eq!(full_report["passed"], true);
    assert!(full_report.get("budget_violations").is_some());

    let summary = run(
        fixture.as_ref(),
        &["check", "--format", "json", "--summary"],
    );
    assert!(summary.status.success(), "{}", stderr(&summary));
    let summary_report = json(&summary);
    assert_eq!(summary_report["passed"], true);
    assert!(summary_report["summary"].is_object());
    assert!(summary_report.get("budget_violations").is_none());
}

#[test]
fn check_missing_scope_is_a_structured_json_command_error() {
    let fixture = Fixture::new("missing-scope", BASE_CONFIG);
    let output = run(
        fixture.as_ref(),
        &["check", "missing.rs", "--format", "json"],
    );

    assert!(!output.status.success());
    let failure = json(&output);
    assert_eq!(failure["passed"], false);
    assert_eq!(failure["stage"], "check");
    assert_eq!(failure["kind"], "command-error");
    assert!(
        failure["message"]
            .as_str()
            .unwrap()
            .contains("Path not found")
    );
}

#[test]
fn diff_without_git_is_a_structured_json_command_error() {
    let fixture = Fixture::new("diff-without-git", BASE_CONFIG);
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    let output = run(fixture.as_ref(), &["check", "--diff", "--format", "json"]);

    assert!(!output.status.success());
    let failure = json(&output);
    assert_eq!(failure["passed"], false);
    assert_eq!(failure["stage"], "check");
    assert_eq!(failure["kind"], "command-error");
    assert!(failure["message"].as_str().unwrap().contains("git status"));
}

#[test]
fn verify_empty_explicit_scope_remains_a_successful_scoped_report() {
    let fixture = Fixture::new("empty-scope", BASE_CONFIG);
    std::fs::create_dir(fixture.0.join("empty")).unwrap();
    let output = run(fixture.as_ref(), &["verify", "empty", "--format", "json"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let report = json(&output);
    assert_eq!(report["passed"], true);
    assert_eq!(report["files_scanned"], 0);
    assert!(
        report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry
                    .as_str()
                    .is_some_and(|text| text.contains("given path(s)"))
            })
    );
}

#[test]
fn scan_unsupported_source_has_consistent_failure_shapes() {
    let fixture = Fixture::new("scan-unsupported", BASE_CONFIG);
    fixture.write(
        "migrations/001_init.sql",
        "create table users (id integer);\n",
    );

    for format in ["json", "agent", "terminal"] {
        let output = run(
            fixture.as_ref(),
            &["scan", "migrations/001_init.sql", "--format", format],
        );
        assert!(!output.status.success(), "{format} unexpectedly passed");
        match format {
            "json" => {
                let report = json(&output);
                assert_eq!(report["passed"], false);
                assert!(
                    report["orchestration_violations"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|violation| violation["step"] == "unsupported-source")
                );
            }
            "agent" => {
                let text = stdout(&output);
                assert!(text.contains("Hardgate Failed"), "{text}");
                assert!(text.contains("unsupported-source"), "{text}");
            }
            _ => {
                let text = stdout(&output);
                assert!(text.contains("error[tool]"), "{text}");
                assert!(text.contains("unsupported-source"), "{text}");
            }
        }
    }
}

#[test]
fn scan_parse_failure_has_consistent_failure_shapes() {
    let fixture = Fixture::new("scan-parse-failure", BASE_CONFIG);
    fixture.write("src/broken.rs", "fn broken( {\n");

    for format in ["json", "agent", "terminal"] {
        let output = run(
            fixture.as_ref(),
            &["scan", "src/broken.rs", "--format", format],
        );
        assert!(!output.status.success(), "{format} unexpectedly passed");
        let text = stdout(&output);
        assert!(text.contains("parse-source"), "{format}: {text}");
        if format == "json" {
            let report: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(report["passed"], false);
            assert!(report["orchestration_violations"].is_array());
        } else if format == "agent" {
            assert!(text.contains("Hardgate Failed"), "{text}");
        } else {
            assert!(text.contains("error[tool]"), "{text}");
        }
    }
}

#[test]
fn verify_empty_mutation_report_list_is_blocking_evidence() {
    let config = BASE_CONFIG.replace(
        "[mutation]\nenabled = false",
        "[mutation]\nenabled = true\nreports = []",
    );
    let fixture = Fixture::new("empty-mutation-reports", &config);
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    let output = run(fixture.as_ref(), &["verify", "--format", "json"]);
    assert!(!output.status.success());
    let report = json(&output);
    assert_eq!(report["passed"], false);
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|violation| {
                violation["step"] == "mutation-report"
                    && violation["command"] == "<empty-report-list>"
            })
    );
}

#[test]
fn analyze_file_content_fails_closed_when_classification_is_invalid() {
    let mut config = HardgateConfig::default();
    config.classification.rules.push(ClassificationRule {
        glob: "[".to_string(),
        role: FileRole::Source,
    });
    let scanner = AntiGamingScanner::new(&config.anti_gaming);
    let invariants = InvariantsChecker::new(&config.invariants.rules);
    let mut report = GateReport::new("analyze-fixture".to_string());

    let functions = analyze_file_content(
        AnalyzeInput {
            path: Path::new("src/value.rs"),
            content: "fn value() {}\n",
            config: &config,
            root: Path::new("."),
            anti_gaming: &scanner,
            invariants: &invariants,
        },
        &mut report,
    );

    assert!(functions.is_empty());
    let failure = report
        .orchestration_violations
        .first()
        .expect("classification errors must become evidence failures");
    assert_eq!(failure.step, "classify-source");
    assert!(failure.output.contains("Invalid classification glob"));
}

#[test]
fn public_output_entrypoints_cover_legacy_and_combined_modes() {
    let report = GateReport::new("output-fixture".to_string());

    output_report(&report, None).unwrap();
    output_report(&report, Some("json")).unwrap();
    for format in ["agent", "summary", "compact", "terminal"] {
        output_report_with_opts(
            &report,
            &OutputOptions {
                format: Some(format.to_string()),
                ..Default::default()
            },
        )
        .unwrap();
    }
    output_report_with_opts(
        &report,
        &OutputOptions {
            format: Some("json".to_string()),
            summary: true,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn public_empty_discovery_printer_distinguishes_scope_and_diff() {
    print_empty_discovery(false, false);
    print_empty_discovery(true, false);
    print_empty_discovery(false, true);
}

#[test]
fn invalid_output_format_is_rejected_before_gate_execution() {
    let fixture = Fixture::new("invalid-format", BASE_CONFIG);
    let output = run(fixture.as_ref(), &["check", "--format", "yaml"]);

    assert!(!output.status.success());
    assert!(stdout(&output).is_empty());
    let message = stderr(&output);
    assert!(message.contains("invalid value"), "{message}");
    assert!(message.contains("yaml"), "{message}");
}
