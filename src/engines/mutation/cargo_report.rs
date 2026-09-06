//! cargo-mutants structured outcomes. Terminal text is never a verdict input.
use super::{
    MutationStats, ReportCategory, as_count, ensure_nonempty, require_array_field, require_object,
};
use anyhow::{Result, bail, ensure};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn parse(value: &Value) -> Result<MutationStats> {
    let root = require_object(value, "cargo-mutants report root")?;
    let outcomes = require_array_field(root, "outcomes", "cargo-mutants report")?;
    ensure!(
        root.get("cargo_mutants_version")
            .and_then(Value::as_str)
            .is_some_and(|version| !version.is_empty()),
        "cargo-mutants report lacks producer version"
    );
    ensure!(
        root.get("end_time").is_some_and(|time| time.is_string()),
        "cargo-mutants report is incomplete: missing end_time"
    );
    let mut stats = MutationStats::default();
    let mut baselines = 0;
    let mut mutants = BTreeSet::new();
    for outcome in outcomes {
        let object = require_object(outcome, "cargo-mutants outcome")?;
        let scenario = object
            .get("scenario")
            .ok_or_else(|| anyhow::anyhow!("cargo-mutants outcome lacks scenario"))?;
        let summary = object
            .get("summary")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("cargo-mutants outcome lacks summary"))?;
        let phases = require_array_field(object, "phase_results", "cargo-mutants outcome")?;
        if scenario.as_str() == Some("Baseline") {
            baselines += 1;
            ensure!(summary == "Success", "cargo-mutants baseline did not pass");
            validate_test(phases, false)?;
        } else {
            ensure!(
                scenario.get("Mutant").is_some_and(Value::is_object),
                "unknown cargo-mutants scenario"
            );
            ensure!(
                mutants.insert(scenario.to_string()),
                "duplicate cargo-mutants scenario"
            );
            stats.add(classify(summary, phases)?)?;
        }
    }
    ensure!(
        baselines == 1,
        "cargo-mutants evidence requires exactly one successful executed baseline"
    );
    for (key, count) in [
        ("total_mutants", stats.total),
        ("caught", stats.killed),
        ("missed", stats.survived),
        ("timeout", stats.timeout),
        ("unviable", stats.unviable),
        ("success", 0),
    ] {
        let declared = root
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("cargo-mutants report lacks {key}"))?;
        ensure!(
            as_count(declared, key)? == count,
            "cargo-mutants {key} disagrees with structured outcomes"
        );
    }
    ensure_nonempty(&stats, "cargo-mutants")?;
    Ok(stats)
}

fn classify(summary: &str, phases: &[Value]) -> Result<ReportCategory> {
    match summary {
        "CaughtMutant" => {
            validate_test(phases, true)?;
            Ok(ReportCategory::Killed)
        }
        "MissedMutant" => {
            validate_test(phases, false)?;
            Ok(ReportCategory::Survived)
        }
        "Timeout" => {
            ensure!(
                phases
                    .iter()
                    .any(|phase| phase.get("process_status").and_then(Value::as_str)
                        == Some("Timeout")),
                "cargo-mutants timeout lacks phase evidence"
            );
            Ok(ReportCategory::Timeout)
        }
        "Unviable" => {
            ensure!(
                phases
                    .iter()
                    .any(
                        |phase| phase.get("phase").and_then(Value::as_str) == Some("Build")
                            && failed(phase.get("process_status"))
                    ),
                "cargo-mutants unviable outcome lacks failed-build evidence"
            );
            Ok(ReportCategory::Unviable)
        }
        _ => bail!("unsupported or incomplete cargo-mutants summary: {summary}"),
    }
}

fn validate_test(phases: &[Value], failure: bool) -> Result<()> {
    let [build, test] = phases else {
        bail!("cargo-mutants accepted outcome requires Build and Test phases");
    };
    ensure!(
        build.get("phase").and_then(Value::as_str) == Some("Build")
            && build.get("process_status").and_then(Value::as_str) == Some("Success"),
        "cargo-mutants build did not succeed"
    );
    ensure!(
        test.get("phase").and_then(Value::as_str) == Some("Test"),
        "cargo-mutants outcome lacks executed tests"
    );
    let argv = test
        .get("argv")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("cargo-mutants test phase lacks command scope"))?;
    ensure!(
        argv.iter()
            .any(|arg| arg.as_str() == Some("test") || arg.as_str() == Some("nextest"))
            && !argv.iter().any(|arg| matches!(
                arg.as_str(),
                Some("--no-run" | "--list" | "--help" | "--version")
            )),
        "cargo-mutants test phase did not execute a test command"
    );
    let status = test.get("process_status");
    ensure!(
        if failure {
            failed(status)
        } else {
            status.and_then(Value::as_str) == Some("Success")
        },
        "cargo-mutants summary disagrees with test status"
    );
    Ok(())
}

fn failed(status: Option<&Value>) -> bool {
    status
        .and_then(|value| value.get("Failure"))
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 0)
}
