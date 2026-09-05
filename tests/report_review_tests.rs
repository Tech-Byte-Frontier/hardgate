#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, assert_status, json, run};
use hardgate::GateReport;
use hardgate::diagnostics::execution::{
    ConfigIdentity, EngineExecution, EngineId, EngineState, ExecutionPlan, ExecutionScope,
};
use hardgate::engines::{BudgetViolation, OrchestrationViolation};
use std::path::PathBuf;

fn report() -> GateReport {
    let mut report = GateReport::new("review".into());
    report.budget_violations.push(BudgetViolation {
        file: "src/large.rs".into(),
        metric: "Physical Lines".into(),
        actual: 600,
        limit: 500,
        message: "too large".into(),
    });
    report.finalize(1, 0, 1);
    report
}

fn save(fixture: &Fixture, name: &str, report: &GateReport) {
    fixture.write(name, &serde_json::to_string(report).unwrap());
}

#[test]
fn inspecting_an_empty_slice_preserves_the_saved_verdict() {
    let fixture = Fixture::new("report-review", "saved-verdict", None);
    save(&fixture, "input.json", &report());
    let output = run(
        fixture.as_ref(),
        &["report", "input.json", "--top", "0", "--json"],
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_eq!(value["passed"], false);
    assert_eq!(value["status"], "violations");
    assert_eq!(value["exit_code"], 1);
    assert!(value["budget_violations"].as_array().unwrap().is_empty());
}

#[test]
fn filtering_and_resaving_an_incomplete_report_preserves_exit_two() {
    let fixture = Fixture::new("report-review", "incomplete-verdict", None);
    let mut input = report();
    input.orchestration_violations.push(OrchestrationViolation {
        step: "coverage-report".into(),
        command: "missing.info".into(),
        exit_code: None,
        output: "missing evidence".into(),
        recommendation: "produce evidence".into(),
    });
    input.finalize(1, 0, 1);
    save(&fixture, "input.json", &input);
    let output = run(
        fixture.as_ref(),
        &[
            "report",
            "input.json",
            "--engine",
            "complexity",
            "--json",
            "--output",
            "slice.json",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["exit_code"], 2);
    let repeated = run(fixture.as_ref(), &["report", "slice.json", "--json"]);
    assert_eq!(repeated.status.code(), Some(2));
    assert_eq!(json(&repeated)["passed"], false);
}

#[test]
fn report_runtime_errors_use_the_requested_json_contract() {
    let fixture = Fixture::new("report-review", "json-errors", None);
    save(&fixture, "input.json", &report());
    for args in [
        vec!["report", "missing.json", "--json"],
        vec!["report", "compare", "input.json", "missing.json", "--json"],
        vec!["report", "input.json", "--engine", "typo", "--json"],
    ] {
        let output = run(fixture.as_ref(), &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stderr.is_empty(), "{args:?}");
        let value = json(&output);
        assert_eq!(value["command"], "report");
        assert_eq!(value["passed"], false);
    }
}

fn execution(selected: bool) -> ExecutionPlan {
    ExecutionPlan {
        command: "check".into(),
        scope: ExecutionScope {
            mode: "full".into(),
            paths: vec![],
        },
        config: ConfigIdentity {
            path: None,
            root: PathBuf::from("/project"),
            policy_sha256: "same".into(),
        },
        engines: vec![EngineExecution {
            id: EngineId::Tests,
            enabled: true,
            selected,
            required_evidence: vec!["test command".into()],
            state: if selected {
                EngineState::Completed
            } else {
                EngineState::Skipped
            },
            reason: None,
        }],
    }
}

#[test]
fn compare_requires_matching_engine_selection_and_execution_metadata() {
    let fixture = Fixture::new("report-review", "scope", None);
    let mut before = report();
    before.execution = Some(execution(true));
    save(&fixture, "before.json", &before);
    for plan in [Some(execution(false)), None] {
        let mut after = before.clone();
        after.execution = plan;
        save(&fixture, "after.json", &after);
        let output = run(
            fixture.as_ref(),
            &["report", "compare", "before.json", "after.json", "--json"],
        );
        let value = json(&output);
        assert_eq!(value["equivalent"], false);
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["command"], "report");
        assert_eq!(value["exit_code"], 1);
    }
}

#[test]
fn verify_and_disabled_mutation_write_the_requested_output_file() {
    let fixture = Fixture::new(
        "report-review",
        "output",
        Some("[gate]\npreset = 'custom'\n[mutation]\nenabled = false\n"),
    );
    fixture.write("src/lib.rs", "pub fn value() -> i32 { 42 }\n");
    for command in ["verify", "mutate"] {
        let name = format!("{command}.json");
        let output = run(fixture.as_ref(), &[command, "--json", "--output", &name]);
        assert_status(&output, true, command);
        let saved =
            std::fs::read_to_string(fixture.as_ref().join(&name)).expect("output must be saved");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&saved).unwrap(),
            json(&output)
        );
    }
}

#[test]
fn comparison_keeps_distinct_functions_and_clone_partners() {
    use hardgate::engines::{CloneViolation, CoverageViolation};
    let mut input = GateReport::new("finding identities".into());
    for (function, partner) in [("first", "src/b.rs"), ("second", "src/c.rs")] {
        input.coverage_violations.push(CoverageViolation {
            file: "src/a.rs".into(),
            function_name: Some(function.into()),
            metric: "CRAP Score".into(),
            actual: 40.0,
            limit: 25.0,
            message: "high risk".into(),
            recommendation: "add tests".into(),
        });
        input.clone_violations.push(CloneViolation {
            file_a: "src/a.rs".into(),
            file_b: partner.into(),
            lines_a: (1, 10),
            lines_b: (1, 10),
            lines: 10,
            tokens: 100,
            fingerprint: "same-content".into(),
            message: "duplicate".into(),
            recommendation: "extract helper".into(),
        });
    }
    input.finalize(3, 2, 1);
    let comparison = hardgate::commands::report::compare::compare_reports(
        &input,
        &input,
        std::path::Path::new("before"),
        std::path::Path::new("after"),
    );
    assert_eq!(comparison.summary.retained, 4);
}

#[cfg(target_os = "linux")]
#[test]
fn comparison_stdout_errors_return_a_command_error_instead_of_panicking() {
    let fixture = Fixture::new("report-review", "stdout-error", None);
    save(&fixture, "input.json", &report());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .current_dir(fixture.as_ref())
        .args(["report", "compare", "input.json", "input.json", "--json"])
        .stdout(
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .unwrap(),
        )
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
}

#[test]
fn scope_advice_adds_only_omitted_flags_and_requires_enabling_evidence() {
    let config =
        "[gate]\npreset = 'custom'\n[coverage]\nenabled = false\n[mutation]\nenabled = false\n";
    for (index, flags, suggestion) in [
        (0, vec![], "add `--all --dead-code`"),
        (1, vec!["--all"], "add `--dead-code`"),
        (2, vec!["--dead-code"], "add `--all`"),
        (3, vec!["--all", "--dead-code"], ""),
    ] {
        let fixture = Fixture::new(
            "report-review",
            &format!("scope-advice-{index}"),
            Some(config),
        );
        fixture.write("src/lib.rs", "pub fn value() -> i32 { 1 }\n");
        let mut args = vec!["check", "--json"];
        args.extend(flags);
        let output = run(fixture.as_ref(), &args);
        assert_status(&output, true, "scope advice");
        let value = json(&output);
        let advice = value["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .find(|s| s.contains("partial gate"))
            .unwrap();
        if suggestion.is_empty() {
            assert!(!advice.contains("add `--"));
        } else {
            assert!(advice.contains(suggestion));
        }
        assert!(advice.contains("enable `[coverage]`"));
        assert!(advice.contains("enable `[mutation]`"));
        assert!(!advice.contains("for complete evidence"));
    }
}

fn compare_plans(before: ExecutionPlan, after: ExecutionPlan) -> serde_json::Value {
    let fixture = Fixture::new("report-review", "metadata", None);
    let mut input = report();
    input.execution = Some(before);
    save(&fixture, "before.json", &input);
    input.execution = Some(after);
    save(&fixture, "after.json", &input);
    let output = run(
        fixture.as_ref(),
        &["report", "compare", "before.json", "after.json", "--json"],
    );
    assert_eq!(output.status.code(), Some(1));
    json(&output)
}

#[test]
fn comparison_distinguishes_policy_scope_command_root_and_required_evidence() {
    let baseline = execution(true);
    let same = compare_plans(baseline.clone(), baseline.clone());
    assert_eq!(same["equivalent"], true);
    let changes: [fn(&mut ExecutionPlan); 6] = [
        |p| p.config.policy_sha256 = "different".into(),
        |p| p.scope.paths.push("src".into()),
        |p| p.scope.mode = "paths".into(),
        |p| p.command = "verify".into(),
        |p| p.config.root = "/other".into(),
        |p| p.engines[0].required_evidence.push("coverage".into()),
    ];
    for (index, change) in changes.into_iter().enumerate() {
        let mut after = baseline.clone();
        change(&mut after);
        let compared = compare_plans(baseline.clone(), after);
        assert_eq!(compared["equivalent"], false, "case {index}");
        let field = if index == 0 {
            "config_differences"
        } else {
            "scope_differences"
        };
        assert_eq!(compared[field].as_array().unwrap().len(), 1, "case {index}");
    }
}

#[test]
fn comparison_refuses_diff_inventory_on_either_side() {
    let mut diff = execution(true);
    diff.scope.mode = "diff".into();
    for (before, after) in [(diff.clone(), execution(true)), (execution(true), diff)] {
        let compared = compare_plans(before, after);
        assert_eq!(compared["equivalent"], false);
        assert!(
            compared["scope_differences"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v
                    .as_str()
                    .unwrap()
                    .contains("complete resolved source inventory"))
        );
    }
}

#[test]
fn comparison_refuses_selected_incomplete_or_skipped_evidence_on_either_side() {
    for state in [EngineState::Incomplete, EngineState::Skipped] {
        let mut partial = execution(true);
        partial.engines[0].state = state;
        for (before, after) in [
            (partial.clone(), execution(true)),
            (execution(true), partial),
        ] {
            let compared = compare_plans(before, after);
            assert_eq!(compared["equivalent"], false);
            assert!(
                compared["scope_differences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v.as_str().unwrap().contains("incomplete or skipped"))
            );
        }
    }
}

#[test]
fn equivalent_terminal_comparison_has_no_warning_but_changed_policy_does() {
    let fixture = Fixture::new("report-review", "terminal-policy", None);
    let mut input = report();
    input.execution = Some(execution(true));
    save(&fixture, "before.json", &input);
    let same = run(
        fixture.as_ref(),
        &["report", "compare", "before.json", "before.json"],
    );
    assert_eq!(same.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&same.stdout).contains("Non-equivalent"));
    input.execution.as_mut().unwrap().config.policy_sha256 = "changed".into();
    save(&fixture, "after.json", &input);
    let changed = run(
        fixture.as_ref(),
        &["report", "compare", "before.json", "after.json"],
    );
    assert_eq!(changed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&changed.stdout).contains("Config policy changed"));
}

#[test]
fn comparison_of_filtered_views_never_claims_equivalent_evidence() {
    let fixture = Fixture::new("report-review", "filtered-compare", None);
    let mut input = report();
    input.execution = Some(execution(true));
    save(&fixture, "input.json", &input);
    let slice = run(
        fixture.as_ref(),
        &[
            "report",
            "input.json",
            "--top",
            "0",
            "--json",
            "--output",
            "slice.json",
        ],
    );
    assert_eq!(slice.status.code(), Some(1));
    for (before, after) in [("input.json", "slice.json"), ("slice.json", "input.json")] {
        let output = run(
            fixture.as_ref(),
            &["report", "compare", before, after, "--json"],
        );
        assert_eq!(output.status.code(), Some(1));
        let compared = json(&output);
        assert_eq!(compared["equivalent"], false);
        assert_eq!(compared["verdict_before"]["total_errors"], 1);
        assert_eq!(compared["verdict_after"]["total_errors"], 1);
        assert!(
            compared["scope_differences"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str().unwrap().contains("Filtered report views"))
        );
    }
}
