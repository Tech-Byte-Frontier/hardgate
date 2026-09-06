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
    let report: GateReport = serde_json::from_value(value.clone())
        .with_context(|| format!("Expected a full gate report in `{}`", path.display()))?;
    ensure!(
        !report.passed || report.total_violations() == 0,
        "Saved report has a passing verdict with blocking findings"
    );
    let mut outcome = CommandOutcome::from_report(&report);
    if !report.passed && matches!(value["status"].as_str(), Some("incomplete" | "error")) {
        outcome = CommandOutcome::Incomplete;
    }
    let original_total_errors = value["inspection"]["original_total_errors"]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or_else(|| report.total_violations());
    let filtered = value["inspection"]["filtered"].as_bool().unwrap_or(false);
    Ok(SavedReport {
        report,
        outcome,
        original_total_errors,
        filtered,
    })
}
