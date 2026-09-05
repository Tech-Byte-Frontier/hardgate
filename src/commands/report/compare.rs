use crate::commands::check::OutputOptions;
use crate::commands::outcome::{CommandOutcome, CommandResult};
use crate::diagnostics::GateReport;
use anyhow::Context;
use colored::*;
use serde::Serialize;
use std::collections::BTreeSet;
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
        let identity = if !v.fingerprint.is_empty() {
            v.fingerprint.clone()
        } else {
            format!("{}:{}-{}", v.file_b.display(), v.lines_b.0, v.tokens)
        };
        findings.insert(FindingRecord {
            engine: "clones",
            file: v.file_a.display().to_string(),
            line: Some(v.lines_a.0),
            identity,
        });
    }
}

fn extract_evidence_findings(report: &GateReport, findings: &mut BTreeSet<FindingRecord>) {
    for v in &report.coverage_violations {
        findings.insert(FindingRecord {
            engine: "coverage",
            file: v.file.display().to_string(),
            line: None,
            identity: v.metric.clone(),
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
    for v in &report.dead_code_violations {
        findings.insert(FindingRecord {
            engine: "dead-code",
            file: v.file.display().to_string(),
            line: v.line_number,
            identity: format!(
                "{}:{}",
                v.violation_type,
                v.symbol.as_deref().unwrap_or_default()
            ),
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
    }
    if before.files_scanned != after.files_scanned {
        scope_differences.push(format!(
            "Files scanned changed (before: {}, after: {})",
            before.files_scanned, after.files_scanned
        ));
    }

    let equivalent = scope_differences.is_empty() && config_differences.is_empty();

    CompareResult {
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
        "diff: +{} added, -{} removed (remediated), {} retained\n",
        cmp.summary.added, cmp.summary.removed, cmp.summary.retained
    ));

    render_differences(&mut out, cmp);
    render_finding_group(
        &mut out,
        format!("Remediated (-{}):", cmp.removed.len()).green(),
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
    let before_content = std::fs::read_to_string(&before_path).with_context(|| {
        format!(
            "Failed to read before report from '{}'",
            before_path.display()
        )
    })?;
    let before_report: GateReport = serde_json::from_str(&before_content).with_context(|| {
        format!(
            "Failed to parse before report JSON from '{}'",
            before_path.display()
        )
    })?;

    let after_content = std::fs::read_to_string(&after_path).with_context(|| {
        format!(
            "Failed to read after report from '{}'",
            after_path.display()
        )
    })?;
    let after_report: GateReport = serde_json::from_str(&after_content).with_context(|| {
        format!(
            "Failed to parse after report JSON from '{}'",
            after_path.display()
        )
    })?;

    let cmp = compare_reports(&before_report, &after_report, &before_path, &after_path);

    let output = if opts.is_json() {
        let json = serde_json::to_string_pretty(&cmp)?;
        format!("{json}\n")
    } else {
        render_compare_terminal(&cmp)
    };

    if let Some(ref path) = opts.output_file {
        crate::commands::outcome::write_atomic_file(path, &output)?;
    }

    print!("{output}");

    Ok(CommandOutcome::from_report(&after_report))
}
