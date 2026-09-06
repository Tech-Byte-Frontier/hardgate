use super::input::read_report;
use crate::commands::check::OutputOptions;
use crate::commands::outcome::{CommandOutcome, CommandResult, write_stdout};
use crate::diagnostics::GateReport;
use crate::diagnostics::execution::{EngineId, ExecutionPlan};
use colored::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct FindingRecord {
    pub engine: &'static str,
    pub file: String,
    pub line: Option<usize>,
    pub identity: String,
}

#[derive(Debug, Serialize)]
pub struct CompareSummary {
    pub added: usize,
    pub removed: usize,
    pub retained: usize,
}

#[derive(Debug, Serialize)]
pub struct VerdictSummary {
    pub passed: bool,
    pub total_errors: usize,
}

#[derive(Debug, Serialize)]
pub struct CompareResult {
    pub schema_version: u32,
    pub command: &'static str,
    pub passed: bool,
    pub status: &'static str,
    pub exit_code: u8,
    pub before_file: String,
    pub after_file: String,
    pub equivalent: bool,
    pub scope_differences: Vec<String>,
    pub config_differences: Vec<String>,
    pub verdict_before: VerdictSummary,
    pub verdict_after: VerdictSummary,
    pub summary: CompareSummary,
    pub added: Vec<FindingRecord>,
    pub removed: Vec<FindingRecord>,
    pub retained: Vec<FindingRecord>,
}

fn extract_static_findings(report: &GateReport, findings: &mut BTreeSet<FindingRecord>) {
    for v in &report.budget_violations {
        findings.insert(FindingRecord {
            engine: "file-budget",
            file: v.file.display().to_string(),
            line: None,
            identity: v.metric.clone(),
        });
    }
    for v in &report.suppression_violations {
        findings.insert(FindingRecord {
            engine: "anti-gaming",
            file: v.file.display().to_string(),
            line: Some(v.line_number),
            identity: v.token.clone(),
        });
    }
    for v in &report.complexity_violations {
        findings.insert(FindingRecord {
            engine: "complexity",
            file: v.file.display().to_string(),
            line: Some(v.line_number),
            identity: format!("{}:{}", v.function_name, v.metric),
        });
    }
    for v in &report.invariant_violations {
        findings.insert(FindingRecord {
            engine: "invariants",
            file: v.file.display().to_string(),
            line: Some(v.line_number),
            identity: v.rule_name.clone(),
        });
    }
    for v in &report.clone_violations {
        let identity = format!(
            "{}:{}:{}-{}:{}",
            v.fingerprint,
            v.file_b.display(),
            v.lines_b.0,
            v.lines_b.1,
            v.tokens
        );
        findings.insert(FindingRecord {
            engine: "clones",
            file: v.file_a.display().to_string(),
            line: Some(v.lines_a.0),
            identity,
        });
    }
}

fn extract_evidence_findings(report: &GateReport, findings: &mut BTreeSet<FindingRecord>) {
    for finding in report
        .tool_diagnostics
        .iter()
        .filter(|finding| finding.blocking)
    {
        findings.insert(FindingRecord {
            engine: "specialist",
            file: finding.file.display().to_string(),
            line: Some(finding.line),
            identity: finding.rule.clone(),
        });
    }
    for v in &report.coverage_violations {
        findings.insert(FindingRecord {
            engine: "coverage",
            file: v.file.display().to_string(),
            line: None,
            identity: format!(
                "{}:{}",
                v.function_name.as_deref().unwrap_or_default(),
                v.metric
            ),
        });
    }
    for v in &report.mutation_violations {
        findings.insert(FindingRecord {
            engine: "mutation",
            file: v.report_file.display().to_string(),
            line: None,
            identity: v.metric.clone(),
        });
    }

    for v in &report.orchestration_violations {
        findings.insert(FindingRecord {
            engine: "orchestration",
            file: v.step.clone(),
            line: None,
            identity: v.command.clone(),
        });
    }
}

pub fn extract_findings(report: &GateReport) -> BTreeSet<FindingRecord> {
    let mut findings = BTreeSet::new();
    extract_static_findings(report, &mut findings);
    extract_evidence_findings(report, &mut findings);
    findings
}

fn comparison_differences(before: &GateReport, after: &GateReport) -> (Vec<String>, Vec<String>) {
    let mut scope_differences = Vec::new();
    let mut config_differences = Vec::new();

    if let (Some(b_exec), Some(a_exec)) = (&before.execution, &after.execution) {
        if b_exec.config.policy_sha256 != a_exec.config.policy_sha256 {
            config_differences.push(format!(
                "Config policy changed (before: {}, after: {})",
                b_exec.config.policy_sha256, a_exec.config.policy_sha256
            ));
        }
        if b_exec.scope.mode != a_exec.scope.mode || b_exec.scope.paths != a_exec.scope.paths {
            scope_differences.push(format!(
                "Evaluation scope changed (before: {} paths: {:?}, after: {} paths: {:?})",
                b_exec.scope.mode, b_exec.scope.paths, a_exec.scope.mode, a_exec.scope.paths
            ));
        }
        if b_exec.command != a_exec.command || b_exec.config.root != a_exec.config.root {
            scope_differences.push("Command or configuration root changed".into());
        }
        if engine_scope(b_exec) != engine_scope(a_exec) {
            scope_differences.push("Engine selection or required evidence changed".into());
        }
        check_evidence_scope(b_exec, a_exec, &mut scope_differences);
    } else {
        scope_differences.push(
            "Execution metadata is missing; equivalent evaluation scope cannot be established"
                .into(),
        );
    }
    if before.files_scanned != after.files_scanned {
        scope_differences.push(format!(
            "Files scanned changed (before: {}, after: {})",
            before.files_scanned, after.files_scanned
        ));
    }

    (scope_differences, config_differences)
}

fn engine_scope(plan: &ExecutionPlan) -> BTreeMap<EngineId, (bool, bool, Vec<String>)> {
    plan.engines
        .iter()
        .map(|engine| {
            (
                engine.id,
                (
                    engine.enabled,
                    engine.selected,
                    engine.required_evidence.clone(),
                ),
            )
        })
        .collect()
}

fn check_evidence_scope(
    before: &ExecutionPlan,
    after: &ExecutionPlan,
    differences: &mut Vec<String>,
) {
    for plan in [before, after] {
        if plan.scope.mode == "diff" {
            differences
                .push("Diff reports do not record a complete resolved source inventory".into());
            break;
        }
    }
    if [before, after].iter().any(|plan| {
        plan.engines.iter().any(|engine| {
            engine.selected
                && matches!(
                    engine.state,
                    crate::diagnostics::execution::EngineState::Incomplete
                        | crate::diagnostics::execution::EngineState::Skipped
                )
        })
    }) {
        differences.push("Selected engine evidence is incomplete or skipped".into());
    }
}

pub fn compare_reports(
    before: &GateReport,
    after: &GateReport,
    before_path: &Path,
    after_path: &Path,
) -> CompareResult {
    let before_findings = extract_findings(before);
    let after_findings = extract_findings(after);

    let added: Vec<_> = after_findings
        .difference(&before_findings)
        .cloned()
        .collect();
    let removed: Vec<_> = before_findings
        .difference(&after_findings)
        .cloned()
        .collect();
    let retained: Vec<_> = before_findings
        .intersection(&after_findings)
        .cloned()
        .collect();

    let (scope_differences, config_differences) = comparison_differences(before, after);

    let equivalent = scope_differences.is_empty() && config_differences.is_empty();

    CompareResult {
        schema_version: 1,
        command: "report",
        passed: after.passed,
        status: CommandOutcome::from_report(after).status(),
        exit_code: CommandOutcome::from_report(after).exit_code(),
        before_file: before_path.display().to_string(),
        after_file: after_path.display().to_string(),
        equivalent,
        scope_differences,
        config_differences,
        verdict_before: VerdictSummary {
            passed: before.passed,
            total_errors: before.total_violations(),
        },
        verdict_after: VerdictSummary {
            passed: after.passed,
            total_errors: after.total_violations(),
        },
        summary: CompareSummary {
            added: added.len(),
            removed: removed.len(),
            retained: retained.len(),
        },
        added,
        removed,
        retained,
    }
}

fn format_verdict_badge(passed: bool, errors: usize) -> String {
    if passed {
        "PASS".green().to_string()
    } else {
        format!("FAIL ({errors} errors)").red().to_string()
    }
}

fn render_differences(out: &mut String, cmp: &CompareResult) {
    if cmp.equivalent {
        return;
    }
    out.push_str(&format!(
        "\n{}\n",
        "warning: Non-equivalent comparison".yellow()
    ));
    for diff in &cmp.scope_differences {
        out.push_str(&format!("  - {diff}\n"));
    }
    for diff in &cmp.config_differences {
        out.push_str(&format!("  - {diff}\n"));
    }
}

fn render_finding_group(out: &mut String, header: ColoredString, items: &[FindingRecord]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("\n{header}\n"));
    for item in items {
        match item.line {
            Some(line) => out.push_str(&format!(
                "  - [{}] {}:{}: {}\n",
                item.engine, item.file, line, item.identity
            )),
            None => out.push_str(&format!(
                "  - [{}] {}: {}\n",
                item.engine, item.file, item.identity
            )),
        }
    }
}

pub fn render_compare_terminal(cmp: &CompareResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "hardgate report compare: {} vs {}\n",
        cmp.before_file, cmp.after_file
    ));
    out.push_str(&format!("{}\n", "-".repeat(70).dimmed()));

    let before_status =
        format_verdict_badge(cmp.verdict_before.passed, cmp.verdict_before.total_errors);
    let after_status =
        format_verdict_badge(cmp.verdict_after.passed, cmp.verdict_after.total_errors);
    out.push_str(&format!(
        "verdict: before: {before_status} -> after: {after_status}\n"
    ));
    out.push_str(&format!(
        "diff: +{} added, -{} removed, {} retained\n",
        cmp.summary.added, cmp.summary.removed, cmp.summary.retained
    ));

    render_differences(&mut out, cmp);
    render_finding_group(
        &mut out,
        format!("Removed findings (-{}):", cmp.removed.len()).green(),
        &cmp.removed,
    );
    render_finding_group(
        &mut out,
        format!("New findings (+{}):", cmp.added.len()).red(),
        &cmp.added,
    );

    out
}

pub fn cmd_report_compare(
    before_path: PathBuf,
    after_path: PathBuf,
    opts: OutputOptions,
) -> CommandResult {
    let before = read_report(&before_path)?;
    let after = read_report(&after_path)?;
    let mut cmp = compare_reports(&before.report, &after.report, &before_path, &after_path);
    cmp.status = after.outcome.status();
    cmp.exit_code = after.outcome.exit_code();
    cmp.verdict_before.total_errors = before.original_total_errors;
    cmp.verdict_after.total_errors = after.original_total_errors;
    if before.filtered || after.filtered {
        cmp.equivalent = false;
        cmp.scope_differences
            .push("Filtered report views do not establish equivalent evaluation scope".into());
    }

    let output = if opts.is_json() {
        let json = serde_json::to_string_pretty(&cmp)?;
        format!("{json}\n")
    } else {
        render_compare_terminal(&cmp)
    };

    if let Some(ref path) = opts.output_file {
        crate::commands::outcome::write_atomic_file(path, &output)?;
    }

    write_stdout(&output)?;

    Ok(after.outcome)
}
