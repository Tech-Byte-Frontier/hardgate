#[path = "report/compare.rs"]
pub mod compare;
mod input;

pub use compare::cmd_report_compare;

use crate::commands::check::{OutputOptions, format_report_with_opts};
use crate::commands::outcome::{CommandResult, write_atomic_file, write_stdout};
use crate::diagnostics::GateReport;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ReportInspectOptions {
    pub file: PathBuf,
    pub engine: Option<String>,
    pub metric: Option<String>,
    pub top: Option<usize>,
    pub output: OutputOptions,
}

pub fn cmd_report_inspect(opts: ReportInspectOptions) -> CommandResult {
    let saved = input::read_report(&opts.file)?;
    let mut report = saved.report;
    let filtered =
        saved.filtered || opts.engine.is_some() || opts.metric.is_some() || opts.top.is_some();

    if let Some(ref engine) = opts.engine {
        filter_by_engine(&mut report, engine)?;
    }

    if let Some(ref metric) = opts.metric {
        filter_by_metric(&mut report, metric);
    }

    if let Some(top) = opts.top {
        filter_by_top(&mut report, top);
    }

    let mut output = format_report_with_opts(&report, &opts.output)?;
    if opts.output.is_json() {
        let mut value: serde_json::Value = serde_json::from_str(&output)?;
        value["command"] = "report".into();
        value["status"] = saved.outcome.status().into();
        value["exit_code"] = saved.outcome.exit_code().into();
        value["inspection"] = serde_json::json!({
            "filtered": filtered,
            "original_total_errors": saved.original_total_errors,
            "displayed_errors": report.total_violations(),
        });
        output = format!("{}\n", serde_json::to_string_pretty(&value)?);
    } else {
        output.push_str(&format!(
            "\nSaved verdict: {} ({} total errors); displaying {} findings.\n",
            saved.outcome.status(),
            saved.original_total_errors,
            report.total_violations()
        ));
    }
    if let Some(path) = &opts.output.output_file {
        write_atomic_file(path, &output)?;
    }
    write_stdout(&output)?;
    Ok(saved.outcome)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterEngine {
    Complexity,
    Budget,
    Suppression,
    Invariant,
    Clone,
    Coverage,
    Mutation,
    DeadCode,
    Orchestration,
}

fn parse_static_engine(norm: &str) -> Option<FilterEngine> {
    match norm {
        "complexity" => Some(FilterEngine::Complexity),
        "budget" | "file-budget" | "file-budgets" => Some(FilterEngine::Budget),
        "suppression" | "suppressions" | "anti-gaming" => Some(FilterEngine::Suppression),
        "invariant" | "invariants" => Some(FilterEngine::Invariant),
        "clone" | "clones" => Some(FilterEngine::Clone),
        _ => None,
    }
}

fn parse_verification_engine(norm: &str) -> Option<FilterEngine> {
    match norm {
        "coverage" => Some(FilterEngine::Coverage),
        "mutation" | "mutation-report" => Some(FilterEngine::Mutation),
        "dead-code" => Some(FilterEngine::DeadCode),
        "orchestration" | "tool" => Some(FilterEngine::Orchestration),
        _ => None,
    }
}

fn filter_by_engine(report: &mut GateReport, engine: &str) -> anyhow::Result<()> {
    let norm = engine.to_ascii_lowercase().replace('_', "-");
    let target = parse_static_engine(&norm).or_else(|| parse_verification_engine(&norm));
    let target = target.ok_or_else(|| anyhow::anyhow!("Unknown report engine `{engine}`"))?;
    retain_static_violations(report, target);
    retain_verification_violations(report, target);
    Ok(())
}

fn retain_static_violations(report: &mut GateReport, target: FilterEngine) {
    if target != FilterEngine::Complexity {
        report.complexity_violations.clear();
    }
    if target != FilterEngine::Budget {
        report.budget_violations.clear();
    }
    if target != FilterEngine::Suppression {
        report.suppression_violations.clear();
    }
    if target != FilterEngine::Invariant {
        report.invariant_violations.clear();
    }
    if target != FilterEngine::Clone {
        report.clone_violations.clear();
    }
}

fn retain_verification_violations(report: &mut GateReport, target: FilterEngine) {
    if target != FilterEngine::Coverage {
        report.coverage_violations.clear();
    }
    if target != FilterEngine::Mutation {
        report.mutation_violations.clear();
    }
    if target != FilterEngine::DeadCode {
        report.dead_code_violations.clear();
    }
    if target != FilterEngine::Orchestration {
        report.orchestration_violations.clear();
    }
}

fn filter_by_metric(report: &mut GateReport, metric: &str) {
    let lower = metric.to_ascii_lowercase();
    report
        .complexity_violations
        .retain(|v| v.metric.to_string().to_ascii_lowercase().contains(&lower));
    report
        .budget_violations
        .retain(|v| v.metric.to_ascii_lowercase().contains(&lower));
    report
        .coverage_violations
        .retain(|v| v.metric.to_ascii_lowercase().contains(&lower));
    report
        .mutation_violations
        .retain(|v| v.metric.to_ascii_lowercase().contains(&lower));
    report
        .dead_code_violations
        .retain(|v| v.violation_type.to_ascii_lowercase().contains(&lower));
    report.suppression_violations.clear();
    report.invariant_violations.clear();
    report.clone_violations.clear();
    report.orchestration_violations.clear();
}

fn filter_by_top(report: &mut GateReport, top: usize) {
    // Tool failures have commands rather than source-file locations. The saved
    // verdict still retains their failure status when selecting top files.
    report.orchestration_violations.clear();
    let mut file_counts: HashMap<PathBuf, usize> = HashMap::new();
    for v in &report.budget_violations {
        *file_counts.entry(v.file.clone()).or_insert(0) += 1;
    }
    for v in &report.suppression_violations {
        *file_counts.entry(v.file.clone()).or_insert(0) += 1;
    }
    for v in &report.complexity_violations {
        *file_counts.entry(v.file.clone()).or_insert(0) += 1;
    }
    for v in &report.invariant_violations {
        *file_counts.entry(v.file.clone()).or_insert(0) += 1;
    }
    for v in &report.clone_violations {
        *file_counts.entry(v.file_a.clone()).or_insert(0) += 1;
        *file_counts.entry(v.file_b.clone()).or_insert(0) += 1;
    }
    for v in &report.coverage_violations {
        *file_counts.entry(v.file.clone()).or_insert(0) += 1;
    }
    for v in &report.mutation_violations {
        *file_counts.entry(v.report_file.clone()).or_insert(0) += 1;
    }
    for v in &report.dead_code_violations {
        *file_counts.entry(v.file.clone()).or_insert(0) += 1;
    }

    let mut ranked: Vec<(PathBuf, usize)> = file_counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_files: HashSet<PathBuf> = ranked.into_iter().take(top).map(|(p, _)| p).collect();

    report
        .budget_violations
        .retain(|v| top_files.contains(&v.file));
    report
        .suppression_violations
        .retain(|v| top_files.contains(&v.file));
    report
        .complexity_violations
        .retain(|v| top_files.contains(&v.file));
    report
        .invariant_violations
        .retain(|v| top_files.contains(&v.file));
    report
        .clone_violations
        .retain(|v| top_files.contains(&v.file_a) || top_files.contains(&v.file_b));
    report
        .coverage_violations
        .retain(|v| top_files.contains(&v.file));
    report
        .mutation_violations
        .retain(|v| top_files.contains(&v.report_file));
    report
        .dead_code_violations
        .retain(|v| top_files.contains(&v.file));
}
