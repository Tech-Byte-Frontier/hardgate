mod output;
use super::CheckKind;
use super::gate_evidence::{
    ChangedLineFilter, GateRun, empty_discovery_advisory, filter_changed_lines,
    run_generated_freshness, run_legacy_ratchet, run_static_gate_or_empty,
};
use super::outcome::CommandResult;
use super::static_gate::StaticRequest;
use super::verify::{
    CoverageScope, CoverageVerification, SourceCoverageRequest, source_files_for_coverage,
    verify_coverage_with_scope, verify_mutation_at,
};
use crate::config::{ConfigContext, HardgateConfig};
use crate::diagnostics::GateReport;
use crate::engines::OrchestrationEngine;
use crate::git_evidence::{ReferenceEvidence, load_reference};
use anyhow::Result;
pub(crate) use output::format_report_with_opts;
pub use output::{
    Emission, emit_gate_report, output_report, output_report_with_opts, print_empty_discovery,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// CLI options for `hardgate check`, including output modes and path scoping.
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    pub format: Option<String>,
    pub diff: bool,
    pub checks: Vec<CheckKind>,
    pub mutation_report: Option<String>,
    pub coverage_report: Option<String>,
    pub json: bool,
    pub compact: bool,
    pub no_snippets: bool,
    pub summary: bool,
    pub paths: Vec<PathBuf>,
    pub display: crate::diagnostics::display::DisplayOptions,
    pub output_file: Option<PathBuf>,
    pub report_json: Option<PathBuf>,
    pub progress: Option<String>,
}

impl CheckOptions {
    fn output_options(&self) -> OutputOptions {
        OutputOptions {
            format: self.format.clone(),
            json: self.json,
            compact: self.compact,
            no_snippets: self.no_snippets,
            summary: self.summary,
            display: self.display.clone(),
            output_file: self.output_file.clone(),
            report_json: self.report_json.clone(),
            progress: self.progress.clone(),
        }
    }
}

/// Resolved output mode shared by `check` and `scan`.
#[derive(Debug, Clone, Default)]
pub struct OutputOptions {
    pub format: Option<String>,
    pub json: bool,
    pub compact: bool,
    pub no_snippets: bool,
    pub summary: bool,
    pub display: crate::diagnostics::display::DisplayOptions,
    pub output_file: Option<PathBuf>,
    pub report_json: Option<PathBuf>,
    pub progress: Option<String>,
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
/// invariants, complexity, clones, and optional coverage and
/// orchestration checks. Exits non-zero when violations are found.
pub fn cmd_check(opts: CheckOptions) -> CommandResult {
    cmd_check_in(opts, &ConfigContext::load(None)?)
}

pub fn cmd_check_in(mut opts: CheckOptions, context: &ConfigContext) -> CommandResult {
    if let (Some(json), Some(rendered)) = (&opts.report_json, &opts.output_file) {
        anyhow::ensure!(
            !crate::commands::outcome::same_output_path(json, rendered)?,
            "--report-json and --output must use different paths"
        );
    }
    context.resolve_gate_paths(&mut opts.paths, &mut opts.coverage_report);
    opts.mutation_report = context.input_report(opts.mutation_report);
    let resolved = super::check_selection::resolved_context(context, &opts)?;
    let context = &resolved;
    let plan = super::execution_plan::gate_plan(
        context,
        super::execution_plan::GateSelection {
            command: "check",
            paths: &opts.paths,
            diff: opts.diff,
            checks: &opts.checks,
            coverage_report: opts.coverage_report.as_deref(),
            mutation_report: opts.mutation_report.as_deref(),
        },
    )?;

    super::execution_failure::run_planned(plan, |plan| execute_check(opts, context, plan))
}

fn execute_check(
    opts: CheckOptions,
    context: &ConfigContext,
    plan: crate::diagnostics::execution::ExecutionPlan,
) -> CommandResult {
    let ratchet_enabled = opts.selects(CheckKind::Policy) && context.config.legacy.ratchet;
    let start_time = Instant::now();
    let root = context.root.as_path();
    let config = &context.config;
    let GateRun {
        mut report,
        files,
        read_results,
        functions,
        ownership,
        empty,
    } = run_static_phase(&opts, context, ratchet_enabled)?;
    super::gate_evidence::describe_execution(&plan, &mut report);
    report.execution = Some(plan);
    if empty && opts.selects(CheckKind::Policy) {
        report
            .advisories
            .push(empty_discovery_advisory(opts.diff, !opts.paths.is_empty()));
    }

    let progress = opts.progress.as_deref();
    emit_progress(
        progress,
        "static_analysis",
        start_time.elapsed().as_millis(),
    );

    let reference_evidence = if ratchet_enabled {
        run_legacy_ratchet(config, root, &mut report)
    } else {
        None
    };

    if opts.selects(CheckKind::Policy) {
        run_generated_freshness(config, root, &mut report);
    }

    run_verification_phase(
        VerificationPhaseContext {
            opts: &opts,
            context,
            coverage: CheckCoverage {
                config,
                diff: opts.diff,
                cli_report: opts.coverage_report.clone(),
                files: &files,
                read_results: &read_results,
                ownership: &ownership,
                functions: &functions,
                reference_evidence: reference_evidence.as_ref(),
                root,
            },
            start_time,
        },
        &mut report,
    )?;

    let elapsed = start_time.elapsed().as_millis();
    emit_progress(progress, "finalization", elapsed);
    emit_gate_report(
        &mut report,
        Emission {
            read_len: read_results.len(),
            fn_len: functions.len(),
            elapsed,
            opts: &opts.output_options(),
        },
    )
}

fn run_static_phase(
    opts: &CheckOptions,
    context: &ConfigContext,
    ratchet_enabled: bool,
) -> Result<GateRun> {
    if !opts.selects(CheckKind::Policy) {
        return Ok(GateRun {
            report: GateReport::new(context.config.gate.name.clone()),
            files: Vec::new(),
            read_results: Vec::new(),
            functions: Vec::new(),
            ownership: Default::default(),
            empty: true,
        });
    }
    let static_diff = opts.diff && !ratchet_enabled;
    run_static_gate_or_empty(StaticRequest {
        config: &context.config,
        root: context.root.as_path(),
        paths: &opts.paths,
        diff: static_diff,
        snippets: opts.display.snippets,
    })
}

struct VerificationPhaseContext<'a> {
    opts: &'a CheckOptions,
    context: &'a ConfigContext,
    coverage: CheckCoverage<'a>,
    start_time: Instant,
}

fn run_verification_phase(
    phase: VerificationPhaseContext<'_>,
    report: &mut GateReport,
) -> Result<()> {
    let opts = phase.opts;
    let progress = opts.progress.as_deref();
    let root = phase.context.root.as_path();
    let config = &phase.context.config;

    emit_progress(
        progress,
        "orchestration",
        phase.start_time.elapsed().as_millis(),
    );
    run_orchestration(config, root, report, opts);
    if opts.selects(CheckKind::Policy) {
        emit_progress(progress, "coverage", phase.start_time.elapsed().as_millis());
        run_check_coverage(&phase.coverage, report)?;
        if config.mutation.enabled {
            emit_progress(progress, "mutation", phase.start_time.elapsed().as_millis());
            verify_mutation_at(config, opts.mutation_report.clone(), report, root);
        }
    }

    report
        .advisories
        .push(super::gate_evidence::check_scope_advisory(config, opts));
    Ok(())
}

fn emit_progress(progress: Option<&str>, stage: &str, elapsed_ms: u128) {
    if progress == Some("jsonl") {
        let _ = writeln!(
            std::io::stderr().lock(),
            "{{\"stage\":\"{stage}\",\"elapsed_ms\":{elapsed_ms}}}"
        );
    }
}

fn run_orchestration(
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
    options: &CheckOptions,
) {
    let engine = OrchestrationEngine::new(&config.orchestration);
    let policy = &config.orchestration;
    let groups = [
        (
            CheckKind::Format,
            "format_check",
            policy.format_check.iter().collect::<Vec<_>>(),
            true,
        ),
        (CheckKind::Lint, "lint", policy.lint.iter().collect(), true),
        (
            CheckKind::Tests,
            "test",
            policy
                .test_cmd
                .iter()
                .chain(&policy.additional_tests)
                .collect(),
            false,
        ),
        (
            CheckKind::Typecheck,
            "typecheck",
            policy
                .typecheck
                .iter()
                .chain(&policy.feature_checks)
                .collect(),
            false,
        ),
    ];
    let mut steps = Vec::new();
    for (kind, step, commands, required) in groups {
        if !options.selects(kind) {
            continue;
        }
        if required && commands.is_empty() {
            super::evidence::record_evidence_failure(
                report,
                true,
                super::evidence::EvidenceFailure {
                    step,
                    target: root,
                    message: format!(
                        "Required {step} command could not be resolved. Run `hardgate init --preview` for tool-specific setup, or configure [orchestration]."
                    ),
                },
            );
        }
        for command in commands {
            steps.push(crate::engines::orchestration::OrchestrationStep {
                step,
                command,
                recommendation: "Resolve the reported project check before acceptance.",
            });
        }
    }
    for result in engine.run_sequence(&steps, root, config) {
        super::specialist::record(report, result, root);
    }
}

struct CheckCoverage<'a> {
    config: &'a HardgateConfig,
    diff: bool,
    cli_report: Option<String>,
    files: &'a [PathBuf],
    read_results: &'a [super::source_snapshot::SharedSource],
    ownership: &'a crate::discovery::rust_ownership::RustOwnership,
    functions: &'a [crate::engines::FunctionMetrics],
    reference_evidence: Option<&'a ReferenceEvidence>,
    root: &'a Path,
}

fn run_check_coverage(request: &CheckCoverage<'_>, report: &mut GateReport) -> Result<()> {
    if !request.config.coverage.enabled {
        return Ok(());
    }
    let coverage_report = request
        .cli_report
        .clone()
        .or_else(|| request.config.coverage.report.clone());
    let source_files = source_files_for_coverage(SourceCoverageRequest {
        files: request.files,
        functions: request.functions,
        root: request.root,
        config: request.config,
        report,
    });
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
                report,
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
            ownership: Some(request.ownership),
            config: request.config,
            root: request.root,
        })?),
        None if request.config.legacy.ratchet => Some(Default::default()),
        None => load_changed_lines_for_coverage(request, report)?,
    };
    verify_coverage_with_scope(
        CoverageVerification {
            config: request.config,
            cli_report: coverage_report,
            functions: request.functions,
            changed_lines: changed_lines.as_ref(),
            report,
        },
        scope,
    );
    Ok(())
}

fn load_changed_lines_for_coverage(
    request: &CheckCoverage<'_>,
    report: &mut GateReport,
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
            ownership: Some(request.ownership),
            config: request.config,
            root: request.root,
        })?)),
        Err(error) => {
            super::evidence::record_evidence_failure(
                report,
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
