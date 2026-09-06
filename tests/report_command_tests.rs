#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, assert_status, json, run, stdout};
use hardgate::GateReport;
use hardgate::engines::{BudgetViolation, ComplexityContribution, ComplexityViolation};
use serde_json::Value;
use std::path::PathBuf;

fn make_complexity(file: &str, func: &str, metric: &str) -> ComplexityViolation {
    ComplexityViolation {
        file: PathBuf::from(file),
        function_name: func.to_string(),
        line_number: 10,
        column_number: 0,
        end_line: 30,
        size: None,
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
        "Statement Count",
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
    assert_status(&output, false, "saved report violations");
    json(&output)
}

#[test]
fn test_report_inspect_filter_by_engine() {
    let parsed = run_inspect_json("filter-engine", &["--engine", "complexity"]);
    assert_eq!(parsed["complexity_violations"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["budget_violations"].as_array().unwrap().len(), 0);
}

#[test]
fn test_report_inspect_filter_by_metric() {
    let parsed = run_inspect_json("filter-metric", &["--metric", "Statement Count"]);
    assert_eq!(parsed["complexity_violations"].as_array().unwrap().len(), 1);
    assert_eq!(
        parsed["complexity_violations"][0]["metric"],
        "Statement Count"
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
    assert!(term_out.contains("+1 added, -1 removed, 2 retained"));
    assert!(term_out.contains("Removed findings"));
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
fn test_check_progress_jsonl() {
    let fixture = Fixture::new("report", "progress-jsonl", None);
    let config = "[gate]\npreset = 'custom'\nstrict = false\n[budgets.files]\nmax_lines = { default = 500 }\n";
    fixture.write("hardgate.toml", config);
    fixture.write("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    let output = run(
        fixture.as_ref(),
        &[
            "check",
            "--checks",
            "policy",
            "--progress",
            "jsonl",
            "--format",
            "json",
        ],
    );
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_str.contains("\"stage\":\"static_analysis\""));
    assert!(stderr_str.contains("\"stage\":\"finalization\""));
    let stdout_str = stdout(&output);
    let parsed: Value = serde_json::from_str(&stdout_str).unwrap();
    assert!(parsed["passed"].as_bool().unwrap());
}
