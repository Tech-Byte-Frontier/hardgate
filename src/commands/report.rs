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
    let original = saved.report.clone();
    let mut report = saved.report;
    let filtered =
        saved.filtered || opts.engine.is_some() || opts.metric.is_some() || opts.top.is_some();

    apply_filters(&mut report, &opts)?;

    report.display = opts.output.display.clone();
    let displayed = crate::diagnostics::display::diagnostics(&report).shown;
    let mut output = if opts.output.is_json() {
        format_report_with_opts(&report, &opts.output)?
    } else if opts.output.is_summary() {
        format!(
            "{}{}",
            original.render_acceptance_context(),
            original.render_summary()
        )
    } else {
        report.render_triage_with_context(&original, opts.output.format.as_deref() == Some("agent"))
    };
    if opts.output.is_json() {
        let mut value: serde_json::Value = serde_json::from_str(&output)?;
        value["command"] = "report".into();
        value["status"] = saved.outcome.status().into();
        value["exit_code"] = saved.outcome.exit_code().into();
        value["summary"] = serde_json::to_value(original.summary())?;
        value["summary"]["total_errors"] = saved.original_total_errors.into();
        value["total"] = saved.original_total_errors.into();
        value["omitted"] = saved.original_total_errors.saturating_sub(displayed).into();
        value["failures"] = serde_json::to_value(original.failure_diagnostics())?;
        value["inspection"] = serde_json::json!({
            "filtered": filtered,
            "original_total_errors": saved.original_total_errors,
            "displayed_errors": displayed,
        });
        output = format!("{}\n", serde_json::to_string_pretty(&value)?);
    } else {
        output.push_str(&format!(
            "\nSaved verdict: {} ({} total errors); displaying {} findings.\n",
            saved.outcome.status(),
            saved.original_total_errors,
            displayed
        ));
    }
    if let Some(path) = &opts.output.output_file {
        write_atomic_file(path, &output)?;
    }
    write_stdout(&output)?;
    Ok(saved.outcome)
}

fn apply_filters(report: &mut GateReport, opts: &ReportInspectOptions) -> anyhow::Result<()> {
    if let Some(engine) = &opts.engine {
        crate::diagnostics::filter::filter_by_engine(report, engine)?;
    }
    if let Some(metric) = &opts.metric {
        filter_by_metric(report, metric);
    }
    if let Some(top) = opts.top {
        filter_by_top(report, top);
    }
    Ok(())
}

fn filter_by_metric(report: &mut GateReport, metric: &str) {
    let lower = metric.to_ascii_lowercase();
    report
        .tool_diagnostics
        .retain(|finding| finding.rule.to_ascii_lowercase().contains(&lower));
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
    for finding in &report.tool_diagnostics {
        *file_counts.entry(finding.file.clone()).or_insert(0) += 1;
    }
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

    let mut ranked: Vec<(PathBuf, usize)> = file_counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_files: HashSet<PathBuf> = ranked.into_iter().take(top).map(|(p, _)| p).collect();
    report
        .tool_diagnostics
        .retain(|finding| top_files.contains(&finding.file));

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
}
