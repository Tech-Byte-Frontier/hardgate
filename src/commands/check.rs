use super::dead_code::run_dead_code_analysis;
use super::gate_evidence::{
    ChangedLineFilter, GateRun, empty_discovery_advisory, filter_changed_lines,
    run_generated_freshness, run_legacy_ratchet, run_static_gate_or_empty,
};
use super::verify::{
    CoverageScope, CoverageVerification, source_files_for_coverage, verify_coverage_with_scope,
    verify_mutation,
};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::engines::OrchestrationEngine;
use crate::git_evidence::{ReferenceEvidence, load_reference};
use anyhow::Result;
use colored::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// CLI options for `hardgate check`, including output modes and path scoping.
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    pub format: Option<String>,
    pub diff: bool,
    pub all: bool,
    pub dead_code: bool,
    pub coverage_report: Option<String>,
    pub json: bool,
    pub compact: bool,
    pub no_snippets: bool,
    pub summary: bool,
    pub paths: Vec<PathBuf>,
}

/// Resolved output mode shared by `check`, `scan`, and `verify`.
#[derive(Debug, Clone, Default)]
pub struct OutputOptions {
    pub format: Option<String>,
    pub json: bool,
    pub compact: bool,
    pub no_snippets: bool,
    pub summary: bool,
}

impl OutputOptions {
    /// True for `--json` or `--format json`.
    pub fn is_json(&self) -> bool {
        self.json || matches!(self.format.as_deref(), Some("json"))
    }
    /// True for `--summary` or `--format summary`.
    pub fn is_summary(&self) -> bool {
        self.summary || matches!(self.format.as_deref(), Some("summary"))
    }
    /// True for `--compact`, `--no-snippets`, or `--format compact`.
    pub fn is_compact(&self) -> bool {
        self.compact || self.no_snippets || matches!(self.format.as_deref(), Some("compact"))
    }
}

/// Run the fast deterministic static gate: budgets, suppressions,
/// invariants, complexity, clones, and optional dead-code, coverage, and
/// orchestration checks. Exits non-zero when violations are found.
pub fn cmd_check(opts: CheckOptions) -> Result<()> {
    let start_time = Instant::now();
    let root = Path::new(".");
    let config = HardgateConfig::load_or_default(None)?;
    let ratchet_enabled = config.legacy.ratchet;
    let static_diff = opts.diff && !ratchet_enabled;

    let GateRun {
        mut report,
        files,
        read_results,
        functions,
        empty,
    } = run_static_gate_or_empty(&config, static_diff, &opts.paths)?;
    if empty {
        report
            .advisories
            .push(empty_discovery_advisory(opts.diff, !opts.paths.is_empty()));
    }

    if opts.dead_code || config.analysis.dead_code.enabled {
        run_dead_code_analysis(&config, &read_results, root, &mut report)?;
    }

    let reference_evidence = if ratchet_enabled {
        run_legacy_ratchet(
            &config,
            root,
            &mut report,
            opts.dead_code || config.analysis.dead_code.enabled,
        )
    } else {
        None
    };

    run_generated_freshness(&config, root, &mut report);

    run_check_coverage(CheckCoverage {
        config: &config,
        diff: opts.diff,
        cli_report: opts.coverage_report.clone(),
        files: &files,
        read_results: &read_results,
        functions: &functions,
        reference_evidence: reference_evidence.as_ref(),
        root,
        report: &mut report,
    })?;

    if config.mutation.enabled {
        verify_mutation(&config, None, &mut report);
    }

    if opts.all {
        let orch = OrchestrationEngine::new(&config.orchestration);
        let (_res, violations) = orch.run_all_checks(root);
        report.orchestration_violations.extend(violations);
    }

    report.advisories.push(check_scope_advisory(&config, &opts));

    let elapsed = start_time.elapsed().as_millis();
    emit_gate_report(
        &mut report,
        Emission {
            read_len: read_results.len(),
            fn_len: functions.len(),
            elapsed,
            opts: &OutputOptions {
                format: opts.format,
                json: opts.json,
                compact: opts.compact,
                no_snippets: opts.no_snippets,
                summary: opts.summary,
            },
        },
    )?;
    Ok(())
}

struct CheckCoverage<'a> {
    config: &'a HardgateConfig,
    diff: bool,
    cli_report: Option<String>,
    files: &'a [PathBuf],
    read_results: &'a [(PathBuf, String)],
    functions: &'a [crate::engines::FunctionMetrics],
    reference_evidence: Option<&'a ReferenceEvidence>,
    root: &'a Path,
    report: &'a mut GateReport,
}

fn run_check_coverage(mut request: CheckCoverage<'_>) -> Result<()> {
    if !request.config.coverage.enabled {
        return Ok(());
    }
    let coverage_report = request
        .cli_report
        .clone()
        .or_else(|| request.config.coverage.report.clone());
    let source_files = source_files_for_coverage(request.files, request.config, request.report);
    let scope = CoverageScope {
        source_files: &source_files,
        root: request.root,
    };
    if !request.diff {
        verify_coverage_with_scope(
            CoverageVerification {
                config: request.config,
                cli_report: coverage_report,
                functions: request.functions,
                changed_lines: None,
                report: request.report,
            },
            scope,
        );
        return Ok(());
    }

    let changed_lines = match request.reference_evidence {
        Some(evidence) => Some(filter_changed_lines(ChangedLineFilter {
            changed_lines: &evidence.change_set.changed_lines,
            selected_files: request.files,
            read_results: request.read_results,
            config: request.config,
            root: request.root,
        })?),
        None if request.config.legacy.ratchet => Some(Default::default()),
        None => load_changed_lines_for_coverage(&mut request)?,
    };
    verify_coverage_with_scope(
        CoverageVerification {
            config: request.config,
            cli_report: coverage_report,
            functions: request.functions,
            changed_lines: changed_lines.as_ref(),
            report: request.report,
        },
        scope,
    );
    Ok(())
}

fn load_changed_lines_for_coverage(
    request: &mut CheckCoverage<'_>,
) -> Result<Option<crate::git_evidence::ChangedLineMap>> {
    let reference = request
        .config
        .legacy
        .reference_branch
        .as_deref()
        .unwrap_or("HEAD");
    match load_reference(request.root, reference) {
        Ok(evidence) => Ok(Some(filter_changed_lines(ChangedLineFilter {
            changed_lines: &evidence.change_set.changed_lines,
            selected_files: request.files,
            read_results: request.read_results,
            config: request.config,
            root: request.root,
        })?)),
        Err(error) => {
            super::evidence::record_evidence_failure(
                request.report,
                true,
                super::evidence::EvidenceFailure {
                    step: "coverage-diff",
                    target: Path::new(reference),
                    message: format!(
                        "Unable to load Git reference evidence for changed-line coverage: {error}"
                    ),
                },
            );
            Ok(Some(Default::default()))
        }
    }
}

fn check_scope_advisory(config: &HardgateConfig, opts: &CheckOptions) -> String {
    let mut omitted = Vec::new();
    if !opts.all {
        omitted.push("configured formatter/linter/test commands");
    }
    if !opts.dead_code && !config.analysis.dead_code.enabled {
        omitted.push("dead-code analysis");
    }
    if !config.coverage.enabled {
        omitted.push("coverage evidence (disabled by policy)");
    }
    if !config.mutation.enabled {
        omitted.push("mutation evidence (disabled by policy)");
    }
    if omitted.is_empty() {
        "This check evaluated every configured report and static/orchestration engine; native mutation execution remains a separate `hardgate mutate` command."
            .to_string()
    } else {
        format!(
            "This is a partial gate; omitted {}. Use `check --all --dead-code`, `verify`, and an enabled `mutate` policy for complete evidence.",
            omitted.join(", ")
        )
    }
}

/// Print the "nothing to check" note, distinguishing scoped runs from diffs.
pub fn print_empty_discovery(diff: bool, scoped: bool) {
    if scoped {
        println!(
            "{} no matching source files detected for the given path(s).",
            "warning:".yellow().bold()
        );
    } else if diff {
        println!(
            "{} no git-modified source files detected to check.",
            "note:".green().bold()
        );
    } else {
        println!(
            "{} no matching source files detected.",
            "warning:".yellow().bold()
        );
    }
}

/// Render `report` with a legacy `format` name (`agent`, `json`, terminal).
/// Prefer [`output_report_with_opts`] for the full flag matrix.
pub fn output_report(report: &GateReport, format: Option<&str>) -> Result<(), serde_json::Error> {
    output_report_with_opts(
        report,
        &OutputOptions {
            format: format.map(|s| s.to_string()),
            json: false,
            compact: false,
            no_snippets: false,
            summary: false,
        },
    )
}

/// Render `report` honoring JSON, agent, summary, compact, and terminal modes.
pub fn output_report_with_opts(
    report: &GateReport,
    opts: &OutputOptions,
) -> Result<(), serde_json::Error> {
    if opts.is_json() {
        if opts.is_summary() {
            println!("{}", report.render_summary_json()?);
        } else {
            println!("{}", report.render_json()?);
        }
        return Ok(());
    }
    match opts.format.as_deref() {
        Some("agent") => print!("{}", report.render_agent()),
        _ if opts.is_summary() => print!("{}", report.render_summary()),
        _ if opts.is_compact() => print!("{}", report.render_compact()),
        _ => print!("{}", report.render_terminal()),
    }
    Ok(())
}

/// Finalize counts, render via [`OutputOptions`], and exit non-zero on
/// failure. Shared by `check`, `scan`, and `verify` so the tail of every
/// gate command stays a single call instead of a duplicated clone block.
pub struct Emission<'a> {
    pub read_len: usize,
    pub fn_len: usize,
    pub elapsed: u128,
    pub opts: &'a OutputOptions,
}

/// Finalize `report` from `emission` counts, render it, and exit(1) on failure.
pub fn emit_gate_report(report: &mut GateReport, emission: Emission) -> anyhow::Result<()> {
    report.finalize(emission.read_len, emission.fn_len, emission.elapsed);
    output_report_with_opts(report, emission.opts)?;
    if !report.passed {
        std::process::exit(1);
    }
    Ok(())
}
