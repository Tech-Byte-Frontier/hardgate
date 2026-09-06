#[path = "support/fs.rs"]
mod fs;

use fs::tempdir;
use hardgate::config::MutationConfig;
use hardgate::engines::{MutationGatekeeper, MutationStats};
use std::path::{Path, PathBuf};

fn gatekeeper() -> MutationGatekeeper {
    gatekeeper_with_floor(85.0)
}

fn gatekeeper_with_floor(min_score: f64) -> MutationGatekeeper {
    MutationGatekeeper::new(&MutationConfig {
        enabled: true,
        min_score: Some(min_score),
        reports: None,
    })
}

fn write_report(root: &Path, name: &str, content: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

// Structured shape emitted by cargo-mutants 27.1.0; these are parser inputs,
// separate from the real producer acceptance trial.
const CARGO_CAUGHT: &str = r#"{
  "cargo_mutants_version":"27.1.0", "end_time":"2026-09-05T00:00:00Z",
  "total_mutants":1,"caught":1,"missed":0,"timeout":0,"unviable":0,"success":0,
  "outcomes":[
    {"scenario":"Baseline","summary":"Success","phase_results":[
      {"phase":"Build","process_status":"Success","argv":["cargo","test","--no-run"]},
      {"phase":"Test","process_status":"Success","argv":["cargo","test"]}
    ]},
    {"scenario":{"Mutant":{"file":"src/lib.rs","replacement":"0"}},"summary":"CaughtMutant","phase_results":[
      {"phase":"Build","process_status":"Success","argv":["cargo","test","--no-run"]},
      {"phase":"Test","process_status":{"Failure":101},"argv":["cargo","test"]}
    ]}
  ]
}"#;

#[test]
fn structured_cargo_outcomes_require_identity_counts_and_executed_phase_scope() {
    let root = tempdir("mutation-structured-fields");
    let original: serde_json::Value = serde_json::from_str(CARGO_CAUGHT).unwrap();
    for pointer in [
        "/outcomes/0/scenario",
        "/outcomes/0/summary",
        "/caught",
        "/outcomes/1/phase_results/1/argv",
    ] {
        let mut report = original.clone();
        let (parent, field) = pointer.rsplit_once('/').unwrap();
        report
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        let path = write_report(&root, "missing.json", &report.to_string());
        assert!(
            gatekeeper().evaluate_report(&path).is_err(),
            "accepted missing {pointer}"
        );
    }
    for arguments in [
        serde_json::json!(["cargo", "test", "--list"]),
        serde_json::json!(["cargo", "check"]),
    ] {
        let mut report = original.clone();
        report["outcomes"][1]["phase_results"][1]["argv"] = arguments;
        let path = write_report(&root, "not-executed.json", &report.to_string());
        assert!(gatekeeper().evaluate_report(&path).is_err());
    }
    let mut report = original;
    report["outcomes"][1]["phase_results"][1]["argv"] =
        serde_json::json!(["cargo", "nextest", "run"]);
    let path = write_report(&root, "nextest.json", &report.to_string());
    assert!(gatekeeper().evaluate_report(&path).unwrap().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn timeout_and_unviable_outcomes_require_matching_phases_and_remain_blocking() {
    let root = tempdir("mutation-phase-outcomes");
    for (summary, counter, phase, status) in [
        ("Timeout", "timeout", "Test", serde_json::json!("Timeout")),
        (
            "Unviable",
            "unviable",
            "Build",
            serde_json::json!({"Failure":101}),
        ),
    ] {
        let mut report: serde_json::Value = serde_json::from_str(CARGO_CAUGHT).unwrap();
        report["caught"] = serde_json::json!(0);
        report[counter] = serde_json::json!(1);
        report["outcomes"][1]["summary"] = serde_json::json!(summary);
        report["outcomes"][1]["phase_results"] =
            serde_json::json!([{"phase":phase,"process_status":status}]);
        let path = write_report(&root, "outcome.json", &report.to_string());
        assert!(!gatekeeper().evaluate_report(&path).unwrap().is_empty());
        report["outcomes"][1]["phase_results"][0]["process_status"] = serde_json::json!("Success");
        std::fs::write(&path, report.to_string()).unwrap();
        assert!(gatekeeper().evaluate_report(&path).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn report_ingestion_api_keeps_parse_failures_distinct_from_mutation_scores() {
    let root = tempdir("mutation-public-ingestion");
    let path = write_report(&root, "mutation.json", CARGO_CAUGHT);
    let mut config = hardgate::config::HardgateConfig::default();
    config.mutation.enabled = true;
    let mut report = hardgate::GateReport::new("fixture".into());
    hardgate::commands::verify::verify_mutation(
        &config,
        Some(path.display().to_string()),
        &mut report,
    );
    assert!(report.mutation_violations.is_empty());
    assert!(report.orchestration_violations.is_empty());
    std::fs::write(&path, "not JSON").unwrap();
    hardgate::commands::verify::verify_mutation(
        &config,
        Some(path.display().to_string()),
        &mut report,
    );
    assert!(report.mutation_violations.is_empty());
    assert_eq!(report.orchestration_violations.len(), 1);
    assert!(
        report.orchestration_violations[0]
            .output
            .contains("Failed to parse required mutation report")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn test_mutation_report_parsers() {
    let tmp = tempdir("mut");
    let keeper = gatekeeper();

    // Stryker shape: 1 killed / 1 survived = 50% < 85% floor.
    let stryker = write_report(
        &tmp,
        "stryker.json",
        r#"{"files": {"a.rs": {"mutants": [{"status": "Killed"}, {"status": "Survived"}]}}}"#,
    );
    let low = keeper.evaluate_report(&stryker).unwrap();
    assert!(low.iter().any(|x| x.metric == "Mutation Kill Rate"));

    // cargo-mutants shape: everything caught = 100%, no violation.
    let caught = write_report(&tmp, "cm.json", CARGO_CAUGHT);
    assert!(keeper.evaluate_report(&caught).unwrap().is_empty());

    // Generic tallies behave the same at 90%+.
    let generic = write_report(
        &tmp,
        "gen.json",
        r#"{"killed": 9, "survived": 1, "timeout": 0}"#,
    );
    assert!(keeper.evaluate_report(&generic).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_zero_viable_score_is_zero() {
    assert_eq!(MutationStats::default().score_percent(), 0.0);

    let stats = MutationStats {
        unviable: 2,
        equivalent: 1,
        total: 3,
        ..Default::default()
    };
    assert_eq!(stats.score_percent(), 0.0);

    let overflowing_viable = MutationStats {
        killed: usize::MAX,
        survived: 1,
        ..Default::default()
    };
    assert_eq!(overflowing_viable.score_percent(), 0.0);
}

#[test]
fn test_mutation_report_rejects_malformed_and_empty_shapes() {
    let tmp = tempdir("mut-invalid");
    let keeper = gatekeeper();
    for (name, content) in [
        ("empty.json", ""),
        ("object.json", "{}"),
        ("stryker-empty.json", r#"{"files": {}}"#),
        (
            "stryker-mutants-empty.json",
            r#"{"files": {"a.rs": {"mutants": []}}}"#,
        ),
        ("cargo-empty.json", r#"{"outcomes": []}"#),
        (
            "cargo-malformed.json",
            r#"{"outcomes": [{"summary": "unknown"}]}"#,
        ),
        (
            "stryker-unknown-status.json",
            r#"{"files": {"a.rs": {"mutants": [{"status": "mystery"}]}}}"#,
        ),
        ("generic-malformed.json", r#"{"killed": "one"}"#),
        (
            "generic-unknown-outcome.json",
            r#"{"killed": 1, "mystery": 1}"#,
        ),
        (
            "generic-unknown-status.json",
            r#"{"killed": 1, "status": "mystery"}"#,
        ),
    ] {
        let path = write_report(&tmp, name, content);
        assert!(
            keeper.evaluate_report(&path).is_err(),
            "{name} should be rejected"
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_mutation_report_rejects_malformed_nested_shapes() {
    let tmp = tempdir("mut-invalid-nested");
    let keeper = gatekeeper();
    for (name, content) in [
        // The root and Stryker `files` field must both be objects.
        ("root-array.json", "[]"),
        ("stryker-files-array.json", r#"{"files": []}"#),
        // Each Stryker file entry must be an object with an array of mutants.
        ("stryker-file-array.json", r#"{"files": {"a.rs": []}}"#),
        (
            "stryker-mutants-object.json",
            r#"{"files": {"a.rs": {"mutants": {}}}}"#,
        ),
        // Every outcome must be an object carrying its format-specific status.
        (
            "stryker-mutant-null.json",
            r#"{"files": {"a.rs": {"mutants": [null]}}}"#,
        ),
        (
            "stryker-mutant-missing-status.json",
            r#"{"files": {"a.rs": {"mutants": [{}]}}}"#,
        ),
        ("cargo-outcomes-object.json", r#"{"outcomes": {}}"#),
        ("cargo-outcome-null.json", r#"{"outcomes": [null]}"#),
        (
            "cargo-outcome-missing-summary.json",
            r#"{"outcomes": [{}]}"#,
        ),
    ] {
        let path = write_report(&tmp, name, content);
        assert!(
            keeper.evaluate_report(&path).is_err(),
            "{name} should be rejected"
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_mutation_report_integrity_outcomes_are_blocking() {
    let tmp = tempdir("mut-integrity");
    let keeper = gatekeeper();
    let report = write_report(
        &tmp,
        "stryker.json",
        r#"{"files":{"a.rs":{"mutants":[
            {"status":"Killed"},
            {"status":"CompileError"},
            {"status":"RuntimeError"},
            {"status":"Equivalent"},
            {"status":"NoCoverage"}
        ]}}}"#,
    );
    let violations = keeper.evaluate_report(&report).unwrap();
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Mutation Compile Errors")
    );
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Mutation Runner Errors")
    );
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Mutation Unviable Mutants")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_timeout_is_always_blocking() {
    let tmp = tempdir("mut-timeout");
    let keeper = gatekeeper();
    let report = write_report(&tmp, "timeout.json", r#"{"killed": 1, "timeout": 1}"#);

    let violations = keeper.evaluate_report(&report).unwrap();
    let timeout = violations
        .iter()
        .find(|violation| violation.metric == "Mutation Timeouts")
        .expect("timeout must be reported");
    assert_eq!(timeout.actual, 1.0);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_equivalent_is_excluded_from_score_but_reported() {
    let tmp = tempdir("mut-equivalent");
    let keeper = gatekeeper();

    let mixed = write_report(
        &tmp,
        "mixed.json",
        r#"{"killed": 9, "survived": 1, "equivalent": 100}"#,
    );
    assert!(keeper.evaluate_report(&mixed).unwrap().is_empty());

    let equivalent_only = write_report(&tmp, "only-equivalent.json", r#"{"equivalent": 2}"#);
    let violations = keeper.evaluate_report(&equivalent_only).unwrap();
    let score = violations
        .iter()
        .find(|violation| violation.metric == "Mutation Kill Rate")
        .expect("zero viable reports must fail the score floor");
    assert_eq!(score.actual, 0.0);
    assert!(score.message.contains("Equivalent: 2"));

    let zero_floor_keeper = gatekeeper_with_floor(0.0);
    assert!(
        !zero_floor_keeper
            .evaluate_report(&equivalent_only)
            .unwrap()
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_generic_counts_reject_overflow_and_mismatched_total() {
    let tmp = tempdir("mut-counts");
    let keeper = gatekeeper();

    let overflow = write_report(
        &tmp,
        "overflow.json",
        &format!(r#"{{"killed": {}, "survived": 1}}"#, usize::MAX),
    );
    assert!(keeper.evaluate_report(&overflow).is_err());

    let mismatch = write_report(
        &tmp,
        "mismatch.json",
        r#"{"killed": 1, "survived": 1, "total": 3}"#,
    );
    assert!(keeper.evaluate_report(&mismatch).is_err());

    let stryker_mismatch = write_report(
        &tmp,
        "stryker-mismatch.json",
        r#"{"files":{"a.rs":{"total":2,"mutants":[{"status":"Killed"}]}}}"#,
    );
    assert!(keeper.evaluate_report(&stryker_mismatch).is_err());

    let cargo_mismatch = write_report(
        &tmp,
        "cargo-mismatch.json",
        r#"{"total":2,"outcomes":[{"summary":"caught"}]}"#,
    );
    assert!(keeper.evaluate_report(&cargo_mismatch).is_err());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_cargo_mutants_require_baseline_executed_tests_and_consistent_counts() {
    let tmp = tempdir("mut-cargo-integrity");
    let keeper = gatekeeper();
    let original: serde_json::Value = serde_json::from_str(CARGO_CAUGHT).unwrap();
    let mut no_baseline = original.clone();
    no_baseline["outcomes"].as_array_mut().unwrap().remove(0);
    let mut no_tests = original.clone();
    no_tests["outcomes"][1]["phase_results"]
        .as_array_mut()
        .unwrap()
        .pop();
    let mut wrong_count = original.clone();
    wrong_count["caught"] = 2.into();
    let mut wrong_status = original.clone();
    wrong_status["outcomes"][1]["phase_results"][1]["process_status"] = "Success".into();
    let mut unfinished = original;
    unfinished["end_time"] = serde_json::Value::Null;
    for (index, invalid) in [no_baseline, no_tests, wrong_count, wrong_status, unfinished]
        .iter()
        .enumerate()
    {
        let path = write_report(&tmp, &format!("invalid-{index}.json"), &invalid.to_string());
        assert!(keeper.evaluate_report(&path).is_err(), "case {index}");
    }
    std::fs::remove_dir_all(tmp).unwrap();
}
