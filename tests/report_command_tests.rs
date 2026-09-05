#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, run, stdout};
use hardgate::GateReport;
use hardgate::engines::{BudgetViolation, ComplexityContribution, ComplexityViolation};
use serde_json::Value;
use std::path::PathBuf;

fn make_complexity(file: &str, func: &str, metric: &str) -> ComplexityViolation {
    ComplexityViolation {
        file: PathBuf::from(file),
        function_name: func.to_string(),
        line_number: 10,
        end_line: 30,
        metric: metric.to_string(),
        actual: 25.0,
        limit: 15.0,
        breakdown: vec![ComplexityContribution {
            line: 15,
            column: 5,
            kind: "nesting".to_string(),
            description: "deep nesting".to_string(),
            score: 5,
        }],
        message: "complexity exceeded".to_string(),
        recommendation: "refactor function".to_string(),
    }
}

fn make_test_report() -> GateReport {
    let mut report = GateReport::new("test-gate".to_string());
    report.complexity_violations.push(make_complexity(
        "src/alpha.rs",
        "handle_req",
        "Cognitive Complexity",
    ));
    report.complexity_violations.push(make_complexity(
        "src/beta.rs",
        "parse_tokens",
        "Cyclomatic Complexity",
    ));
    report.budget_violations.push(BudgetViolation {
        file: PathBuf::from("src/alpha.rs"),
        metric: "Physical Lines (.rs)".to_string(),
        actual: 550,
        limit: 499,
        message: "file too large".to_string(),
    });
    report.finalize(2, 2, 50);
    report
}

fn run_inspect_json(case_name: &str, flags: &[&str]) -> Value {
    let fixture = Fixture::new("report", case_name, None);
    let report = make_test_report();
    let report_json = serde_json::to_string(&report).unwrap();
    fixture.write("gate-report.json", &report_json);

    let mut args = vec!["report", "gate-report.json"];
    args.extend_from_slice(flags);
    args.extend_from_slice(&["--format", "json"]);
    let output = run(fixture.as_ref(), &args);
    serde_json::from_str(&stdout(&output)).unwrap()
}

#[test]
fn test_report_inspect_filter_by_engine() {
    let parsed = run_inspect_json("filter-engine", &["--engine", "complexity"]);
    assert_eq!(parsed["complexity_violations"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["budget_violations"].as_array().unwrap().len(), 0);
}

#[test]
fn test_report_inspect_filter_by_metric() {
    let parsed = run_inspect_json("filter-metric", &["--metric", "Cognitive Complexity"]);
    assert_eq!(parsed["complexity_violations"].as_array().unwrap().len(), 1);
    assert_eq!(
        parsed["complexity_violations"][0]["metric"],
        "Cognitive Complexity"
    );
    assert_eq!(parsed["budget_violations"].as_array().unwrap().len(), 0);
}

#[test]
fn test_report_inspect_filter_by_top() {
    // alpha.rs has 2 violations (complexity + budget), beta.rs has 1 violation
    let parsed = run_inspect_json("filter-top", &["--top", "1"]);
    let complexity = parsed["complexity_violations"].as_array().unwrap();
    assert_eq!(complexity.len(), 1);
    assert!(complexity[0]["file"].as_str().unwrap().contains("alpha.rs"));
    assert_eq!(parsed["budget_violations"].as_array().unwrap().len(), 1);
}

#[test]
fn test_report_compare_json_and_terminal() {
    let fixture = Fixture::new("report", "compare", None);
    let before = make_test_report();

    // In after report, remove beta.rs complexity, add gamma.rs budget violation
    let mut after = GateReport::new("test-gate".to_string());
    after
        .complexity_violations
        .push(before.complexity_violations[0].clone());
    after
        .budget_violations
        .push(before.budget_violations[0].clone());
    after.budget_violations.push(BudgetViolation {
        file: PathBuf::from("src/gamma.rs"),
        metric: "Physical Lines (.rs)".to_string(),
        actual: 600,
        limit: 499,
        message: "file too large".to_string(),
    });
    after.finalize(3, 2, 60);

    fixture.write("before.json", &serde_json::to_string(&before).unwrap());
    fixture.write("after.json", &serde_json::to_string(&after).unwrap());

    // JSON comparison test
    let output_json = run(
        fixture.as_ref(),
        &[
            "report",
            "compare",
            "before.json",
            "after.json",
            "--format",
            "json",
        ],
    );
    let parsed: Value = serde_json::from_str(&stdout(&output_json)).unwrap();
    assert_eq!(parsed["summary"]["added"], 1);
    assert_eq!(parsed["summary"]["removed"], 1);
    assert_eq!(parsed["summary"]["retained"], 2);
    assert_eq!(parsed["added"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["removed"].as_array().unwrap().len(), 1);

    // Terminal comparison test
    let output_term = run(
        fixture.as_ref(),
        &["report", "compare", "before.json", "after.json"],
    );
    let term_out = stdout(&output_term);
    assert!(term_out.contains("+1 added, -1 removed (remediated), 2 retained"));
    assert!(term_out.contains("Remediated"));
    assert!(term_out.contains("New findings"));
}

#[test]
fn test_atomic_output_file_writing() {
    let fixture = Fixture::new("report", "atomic-output", None);
    let report = make_test_report();
    fixture.write("report.json", &serde_json::to_string(&report).unwrap());

    let out_target = fixture.as_ref().join("nested/dir/sliced.json");
    let output = run(
        fixture.as_ref(),
        &[
            "report",
            "report.json",
            "--engine",
            "complexity",
            "--format",
            "json",
            "--output",
            out_target.to_str().unwrap(),
        ],
    );
    assert!(output.status.success() || output.status.code() == Some(1));
    assert!(
        out_target.exists(),
        "target output file must be created atomically"
    );
    let content = std::fs::read_to_string(&out_target).unwrap();
    let parsed: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["complexity_violations"].as_array().unwrap().len(), 2);
}

#[test]
fn test_mutate_summary_and_atomic_output() {
    let fixture = Fixture::new("report", "mutate-summary", None);
    let dirty = "// current uncommitted input\npub fn compute(value: i32) -> i32 { value + 1 }\n";
    fixture.write("sample.rs", dirty);
    fixture.write("untracked.txt", dirty);
    fixture.write(
        "hardgate.toml",
        "[gate]\npreset = 'custom'\n[mutation]\nenabled = true\nmin_score = 85.0\n",
    );
    let script =
        "if cmp -s sample.rs untracked.txt; then exit 0; fi\necho 'assertion failed'; exit 1\n";
    fixture.write("test.sh", script);

    let out_json = fixture.as_ref().join("nested/mutate-summary.json");
    let output = run(
        fixture.as_ref(),
        &[
            "mutate",
            "--scoped",
            "sample.rs",
            "--test-cmd",
            "sh test.sh",
            "--max-mutants",
            "1",
            "--timeout",
            "1",
            "--json",
            "--summary",
            "--output",
            out_json.to_str().unwrap(),
        ],
    );
    assert!(output.status.success());
    assert!(out_json.exists());
    let json_content = std::fs::read_to_string(&out_json).unwrap();
    let parsed: Value = serde_json::from_str(&json_content).unwrap();
    assert_eq!(parsed["command"], "mutate");
    assert_eq!(parsed["status"], "passed");
    assert_eq!(parsed["all_sources_restored"], true);
    assert!(parsed["survivors"].is_array());
}

#[test]
fn test_check_progress_jsonl() {
    let fixture = Fixture::new("report", "progress-jsonl", None);
    let config = "[gate]\npreset = 'custom'\nstrict = false\n[budgets.files]\nmax_lines = { default = 500 }\n";
    fixture.write("hardgate.toml", config);
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    let output = run(
        fixture.as_ref(),
        &["check", "--progress", "jsonl", "--format", "json"],
    );
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_str.contains("\"stage\":\"static_analysis\""));
    assert!(stderr_str.contains("\"stage\":\"finalization\""));
    let stdout_str = stdout(&output);
    let parsed: Value = serde_json::from_str(&stdout_str).unwrap();
    assert!(parsed["passed"].as_bool().unwrap());
}
