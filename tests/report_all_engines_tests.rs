#[path = "common/cli.rs"]
mod cli;
use cli::{Fixture, assert_status, json, run};
use hardgate::GateReport;
use serde_json::{Value, json as value};

const FIELDS: [&str; 9] = [
    "budget_violations",
    "suppression_violations",
    "complexity_violations",
    "invariant_violations",
    "clone_violations",
    "coverage_violations",
    "mutation_violations",
    "dead_code_violations",
    "orchestration_violations",
];

fn all_findings() -> GateReport {
    let mut report = serde_json::to_value(GateReport::new("all engines".into())).unwrap();
    let records = [
        value!({"file":"src/a.rs","metric":"budget keep","actual":2,"limit":1,"message":"budget"}),
        value!({"file":"src/a.rs","line_number":1,"token":"ignore","line_content":"ignore","message":"suppression"}),
        value!({"file":"src/a.rs","function_name":"work","line_number":1,"end_line":2,"metric":"complexity keep","actual":2.0,"limit":1.0,"breakdown":[],"message":"complexity","recommendation":"split"}),
        value!({"file":"src/a.rs","line_number":1,"rule_name":"boundary","violation_type":"import","offending_target":"other","line_content":"import","message":"invariant"}),
        value!({"file_a":"src/b.rs","file_b":"src/a.rs","lines_a":[1,2],"lines_b":[1,2],"tokens":30,"lines":2,"fingerprint":"clone","message":"clone","recommendation":"extract"}),
        value!({"file":"src/a.rs","function_name":"work","metric":"coverage keep","actual":50.0,"limit":95.0,"message":"coverage","recommendation":"test"}),
        value!({"report_file":"src/a.rs","metric":"mutation keep","actual":50.0,"limit":85.0,"message":"mutation","recommendation":"test"}),
        value!({"file":"src/a.rs","line_number":1,"symbol":"work","violation_type":"dead keep","message":"dead code","recommendation":"remove"}),
        value!({"step":"lint","command":"lint","exit_code":1,"output":"lint failed","recommendation":"fix"}),
    ];
    for (field, record) in FIELDS.iter().zip(records) {
        report[field] = value!([record]);
    }
    let mut report: GateReport = serde_json::from_value(report).unwrap();
    report.finalize(2, 1, 1);
    report
}

fn saved() -> Fixture {
    let fixture = Fixture::new("report-all-engines", "filter", None);
    fixture.write(
        "input.json",
        &serde_json::to_string(&all_findings()).unwrap(),
    );
    fixture
}

fn inspect(fixture: &Fixture, flag: &str, filter: &str) -> Value {
    let output = run(
        fixture.as_ref(),
        &["report", "input.json", flag, filter, "--json"],
    );
    assert_status(&output, false, "saved report remains a policy failure");
    assert_eq!(output.status.code(), Some(1));
    let report = json(&output);
    assert_eq!(report["inspection"]["original_total_errors"], 9);
    report
}

#[test]
fn every_engine_filter_preserves_only_its_saved_findings_and_original_verdict() {
    let fixture = saved();
    let aliases = [
        "file_budgets",
        "anti-gaming",
        "complexity",
        "invariants",
        "clones",
        "coverage",
        "mutation_report",
        "dead_code",
        "tool",
    ];
    for (selected, alias) in aliases.iter().enumerate() {
        let report = inspect(&fixture, "--engine", alias);
        for (index, field) in FIELDS.iter().enumerate() {
            assert_eq!(
                report[field].as_array().unwrap().len(),
                usize::from(index == selected),
                "{alias}: {field}"
            );
        }
    }
}

#[test]
fn metric_and_top_filters_cover_evidence_and_clone_partner_locations() {
    let fixture = saved();
    for (metric, selected) in [
        ("budget", 0),
        ("complexity", 2),
        ("coverage", 5),
        ("mutation", 6),
        ("dead", 7),
    ] {
        let report = inspect(&fixture, "--metric", metric);
        for (index, field) in FIELDS.iter().enumerate() {
            assert_eq!(
                report[field].as_array().unwrap().len(),
                usize::from(index == selected),
                "{metric}: {field}"
            );
        }
    }
    for top in ["0", "1", "2"] {
        let report = inspect(&fixture, "--top", top);
        for (index, field) in FIELDS.iter().enumerate() {
            assert_eq!(
                report[field].as_array().unwrap().len(),
                usize::from(top != "0" && index != 8),
                "top={top}: {field}"
            );
        }
    }
}

#[test]
fn comparison_records_each_engine_without_claiming_equivalent_missing_metadata() {
    let fixture = saved();
    let mut empty = GateReport::new("empty".into());
    empty.finalize(2, 1, 1);
    fixture.write("empty.json", &serde_json::to_string(&empty).unwrap());
    for (before, after, added, removed) in [
        ("input.json", "empty.json", 0, 9),
        ("empty.json", "input.json", 9, 0),
        ("input.json", "input.json", 0, 0),
    ] {
        let output = run(
            fixture.as_ref(),
            &["report", "compare", before, after, "--json"],
        );
        let report = json(&output);
        assert_eq!(report["equivalent"], false);
        assert_eq!(report["summary"]["added"], added);
        assert_eq!(report["summary"]["removed"], removed);
    }
}

#[test]
fn terminal_comparison_renders_added_and_removed_findings_and_saves_exact_output() {
    let fixture = saved();
    let empty = GateReport::new("empty".into());
    fixture.write("empty.json", &serde_json::to_string(&empty).unwrap());
    for (before, after, group) in [
        ("input.json", "empty.json", "Removed findings"),
        ("empty.json", "input.json", "New findings"),
    ] {
        let output = run(
            fixture.as_ref(),
            &[
                "report",
                "compare",
                before,
                after,
                "--output",
                "comparison.txt",
            ],
        );
        assert_eq!(
            output.status.code(),
            Some(if after == "input.json" { 1 } else { 0 })
        );
        let rendered = String::from_utf8(output.stdout).unwrap();
        assert!(rendered.contains(group), "{rendered}");
        assert!(rendered.contains("src/a.rs:1:"), "{rendered}");
        assert!(rendered.contains("[file-budget] src/a.rs:"), "{rendered}");
        assert!(rendered.contains("Non-equivalent comparison"), "{rendered}");
        assert_eq!(
            std::fs::read_to_string(fixture.as_ref().join("comparison.txt")).unwrap(),
            rendered
        );
    }
}
