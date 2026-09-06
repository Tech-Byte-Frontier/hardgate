use super::OutputOptions;
use crate::commands::outcome::{CommandOutcome, CommandResult, write_stdout};
use crate::diagnostics::GateReport;
use anyhow::Result;

/// Print the "nothing to check" note, distinguishing scoped runs from diffs.
pub fn print_empty_discovery(diff: bool, scoped: bool) -> Result<()> {
    let note = crate::commands::gate_evidence::empty_discovery_advisory(diff, scoped);
    write_stdout(&format!("{note}\n"))?;
    Ok(())
}

/// Render `report` with a legacy `format` name (`agent`, `json`, terminal).
/// Prefer [`output_report_with_opts`] for the full flag matrix.
pub fn output_report(report: &GateReport, format: Option<&str>) -> Result<()> {
    output_report_with_opts(
        report,
        &OutputOptions {
            format: format.map(|s| s.to_string()),
            json: false,
            compact: false,
            no_snippets: false,
            summary: false,
            display: Default::default(),
            ..Default::default()
        },
    )
}

/// Render `report` honoring JSON, agent, summary, compact, and terminal modes.
pub fn output_report_with_opts(report: &GateReport, opts: &OutputOptions) -> Result<()> {
    let output = format_report_with_opts(report, opts)?;
    if let Some(ref path) = opts.output_file {
        crate::commands::outcome::write_atomic_file(path, &output)?;
    }
    write_stdout(&output)?;
    Ok(())
}

pub(crate) fn format_report_with_opts(report: &GateReport, opts: &OutputOptions) -> Result<String> {
    let mut owned;
    let report = if report.display.snippets != opts.display.snippets
        || report.display.max_diagnostics != opts.display.max_diagnostics
    {
        owned = report.clone();
        owned.display = opts.display.clone();
        &owned
    } else {
        report
    };
    let mut output = if opts.is_json() {
        json_report(report, opts)?
    } else {
        human_report(report, opts)
    };
    if !opts.is_json() {
        if report
            .execution
            .as_ref()
            .is_some_and(|plan| plan.is_partial())
        {
            output.insert_str(
                0,
                "Partial check: this result does not establish complete project acceptance.\n",
            );
        }
        crate::commands::outcome::append_scan_metrics(&mut output, &report.functions);
    }
    Ok(output)
}

fn human_report(report: &GateReport, opts: &OutputOptions) -> String {
    if !opts.is_summary() && (opts.display.max_diagnostics.is_some() || opts.display.snippets) {
        let display = crate::diagnostics::display::diagnostics(report);
        return format!(
            "{}{}",
            report.render_summary(),
            crate::diagnostics::display::render_diagnostics(&display)
        );
    }
    match opts.format.as_deref() {
        Some("agent") => report.render_agent(),
        _ if opts.is_summary() => report.render_summary(),
        _ if opts.is_compact() => report.render_compact(),
        _ => report.render_terminal(),
    }
}

fn json_report(report: &GateReport, opts: &OutputOptions) -> Result<String> {
    let json = if opts.is_summary() {
        report.render_summary_json()?
    } else {
        report.render_json()?
    };
    Ok(format!("{json}\n"))
}

/// Finalize counts, render via [`OutputOptions`], and exit non-zero on
/// failure through a typed outcome. Shared by `check` and `scan` so the tail of every
/// gate command stays a single call instead of a duplicated clone block.
pub struct Emission<'a> {
    pub read_len: usize,
    pub fn_len: usize,
    pub elapsed: u128,
    pub opts: &'a OutputOptions,
}

/// Finalize and render without terminating the caller process.
pub fn emit_gate_report(report: &mut GateReport, emission: Emission) -> CommandResult {
    report.display = emission.opts.display.clone();
    report.finalize(emission.read_len, emission.fn_len, emission.elapsed);
    output_report_with_opts(report, emission.opts)?;
    Ok(CommandOutcome::from_report(report))
}
