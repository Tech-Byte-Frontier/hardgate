use crate::commands::outcome::CommandOutcome;
use crate::diagnostics::GateReport;
use anyhow::{Context, Result, ensure};
use std::path::Path;

pub(super) struct SavedReport {
    pub report: GateReport,
    pub outcome: CommandOutcome,
    pub original_total_errors: usize,
    pub filtered: bool,
}

pub(super) fn read_report(path: &Path) -> Result<SavedReport> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read report file `{}`", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("Failed to parse report JSON from `{}`", path.display()))?;
    ensure!(
        value.get("schema_version").is_none_or(|v| v == 1),
        "Unsupported report schema version"
    );
    ensure!(
        value
            .get("dead_code_violations")
            .is_none_or(|findings| { findings.as_array().is_some_and(Vec::is_empty) }),
        "This report contains removed dead-code findings; inspect it with the producing Hardgate version"
    );
    let mut report: GateReport = serde_json::from_value(value.clone())
        .with_context(|| format!("Expected a full gate report in `{}`", path.display()))?;
    ensure!(
        !report.passed || report.total_violations() == 0,
        "Saved report has a passing verdict with blocking findings"
    );
    restore_excerpts(&value, &mut report)?;
    let mut outcome = CommandOutcome::from_report(&report);
    if !report.passed && matches!(value["status"].as_str(), Some("incomplete" | "error")) {
        outcome = CommandOutcome::Incomplete;
    }
    let original_total_errors = value["inspection"]["original_total_errors"]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .or_else(|| {
            value["summary"]["total_errors"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
        })
        .unwrap_or_else(|| report.total_violations());
    let filtered = value["inspection"]["filtered"].as_bool().unwrap_or(false)
        || value["omitted"].as_u64().is_some_and(|omitted| omitted > 0)
        || original_total_errors > report.total_violations();
    if let Some(failures) = value.get("failures") {
        report.saved_failures = serde_json::from_value(failures.clone())?;
    }
    if let Some(summary) = value.get("summary") {
        report.saved_summary = Some(serde_json::from_value(summary.clone())?);
    }
    report.saved_outcome = Some(outcome);
    Ok(SavedReport {
        report,
        outcome,
        original_total_errors,
        filtered,
    })
}

fn restore_excerpts(value: &serde_json::Value, report: &mut GateReport) -> Result<()> {
    for diagnostic in value["diagnostics"].as_array().into_iter().flatten() {
        for excerpt in diagnostic["excerpts"].as_array().into_iter().flatten() {
            report
                .saved_excerpts
                .push(serde_json::from_value(excerpt.clone())?);
        }
    }
    Ok(())
}
