use super::check::{Emission, OutputOptions, emit_gate_report};
use super::outcome::CommandResult;
use super::static_gate::{AnalyzeInput, analyze_file_content};
use crate::config::ConfigContext;
use crate::diagnostics::GateReport;
use crate::engines::{AntiGamingScanner, InvariantsChecker};
use anyhow::Context;
use std::fs;
use std::path::Path;

/// Inspect one file's AST metrics, suppressions, and budgets, then render
/// and exit non-zero when violations are found.
pub fn cmd_scan(file_path: &Path, opts: OutputOptions) -> CommandResult {
    cmd_scan_in(file_path, opts, &ConfigContext::load(None)?)
}

pub fn cmd_scan_in(
    file_path: &Path,
    opts: OutputOptions,
    context: &ConfigContext,
) -> CommandResult {
    let file_path = context.input_path(file_path);
    let plan = super::execution_plan::gate_plan(
        context,
        super::execution_plan::GateSelection {
            command: "scan",
            paths: std::slice::from_ref(&file_path),
            diff: false,
            checks: &[super::CheckKind::Policy],
            coverage_report: None,
            mutation_report: None,
        },
    )?;
    super::execution_failure::run_planned(plan, |plan| {
        execute_scan(&file_path, opts, context, plan)
    })
}

fn execute_scan(
    file_path: &Path,
    opts: OutputOptions,
    context: &ConfigContext,
    plan: crate::diagnostics::execution::ExecutionPlan,
) -> CommandResult {
    let start_time = std::time::Instant::now();
    let root = context.root.as_path();
    let config = &context.config;
    if !file_path.exists() {
        anyhow::bail!("File not found: {:?}", file_path);
    }
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {:?}", file_path))?;
    let mut report = GateReport::new(config.gate.name.clone());
    report.execution = Some(plan);
    let scanner = AntiGamingScanner::new(&config.anti_gaming);
    let invariants = InvariantsChecker::new(&config.invariants.rules);
    let functions = analyze_file_content(
        AnalyzeInput {
            path: file_path,
            content: &content,
            config,
            root,
            anti_gaming: &scanner,
            invariants: &invariants,
        },
        &mut report,
    );

    if opts.display.snippets {
        let relative = file_path.strip_prefix(root).unwrap_or(file_path);
        report
            .source_text
            .insert(relative.to_path_buf(), std::sync::Arc::from(content));
    }
    let fn_len = functions.len();
    report.functions = functions;
    emit_gate_report(
        &mut report,
        Emission {
            read_len: 1,
            fn_len,
            elapsed: start_time.elapsed().as_millis(),
            opts: &opts,
        },
    )
}

/// Backwards-compatible helper for callers passing `format: Option<&str>`.
/// Prefer [`cmd_scan`] with [`OutputOptions`] for the full flag matrix.
pub fn cmd_scan_with_format(file_path: &Path, format: Option<&str>) -> CommandResult {
    cmd_scan(
        file_path,
        OutputOptions {
            format: format.map(|s| s.to_string()),
            ..Default::default()
        },
    )
}
