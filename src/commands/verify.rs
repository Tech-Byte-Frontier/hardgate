use super::check::{Emission, OutputOptions, emit_gate_report, print_empty_discovery};
use super::evidence::{EvidenceFailure, record_evidence_failure};
use super::static_gate::run_static_gate_scoped;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::engines::{CoverageScorer, FunctionMetrics, MutationGatekeeper};
use crate::git_evidence::ChangedLineMap;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// CLI options for `hardgate verify`, including output modes and path scoping.
#[derive(Debug, Clone, Default)]
pub struct VerifyOptions {
    pub coverage_report: Option<String>,
    pub mutation_report: Option<String>,
    pub format: Option<String>,
    pub json: bool,
    pub compact: bool,
    pub no_snippets: bool,
    pub summary: bool,
    pub paths: Vec<PathBuf>,
}

/// Run static gates plus coverage and mutation report evaluation.
/// Exits non-zero when violations are found.
pub fn cmd_verify(opts: VerifyOptions) -> Result<()> {
    let start_time = Instant::now();
    let config = HardgateConfig::load_or_default(None)?;
    let scoped = !opts.paths.is_empty();

    let Some((mut report, _files, read_results, functions)) =
        run_static_gate_scoped(&config, false, &opts.paths)?
    else {
        print_empty_discovery(false, scoped);
        return Ok(());
    };

    verify_coverage(
        &config,
        opts.coverage_report.clone(),
        &functions,
        &mut report,
    );
    verify_mutation(&config, opts.mutation_report.clone(), &mut report);

    emit_gate_report(
        &mut report,
        Emission {
            read_len: read_results.len(),
            fn_len: functions.len(),
            elapsed: start_time.elapsed().as_millis(),
            opts: &OutputOptions {
                format: opts.format.clone(),
                json: opts.json,
                compact: opts.compact,
                no_snippets: opts.no_snippets,
                summary: opts.summary,
            },
        },
    );
    Ok(())
}

/// Backwards-compatible shim for callers using the pre-struct signature.
pub fn cmd_verify_legacy(
    coverage_report: Option<String>,
    mutation_report: Option<String>,
    format: Option<&str>,
) -> Result<()> {
    cmd_verify(VerifyOptions {
        coverage_report,
        mutation_report,
        format: format.map(|s| s.to_string()),
        ..Default::default()
    })
}

/// Request data for a coverage verification run.
pub struct CoverageVerification<'a> {
    pub config: &'a HardgateConfig,
    pub cli_report: Option<String>,
    pub functions: &'a [FunctionMetrics],
    /// Normalized changed executable-line candidates; `None` keeps full mode.
    pub changed_lines: Option<&'a ChangedLineMap>,
    pub report: &'a mut GateReport,
}

/// Ingest an lcov report and flag functions breaching coverage/CRAP floors.
pub fn verify_coverage(
    config: &HardgateConfig,
    cli_report: Option<String>,
    functions: &[FunctionMetrics],
    report: &mut GateReport,
) {
    verify_coverage_with_diff(CoverageVerification {
        config,
        cli_report,
        functions,
        changed_lines: None,
        report,
    });
}

/// Ingest an lcov report and evaluate either the full project or supplied
/// changed executable lines.
pub fn verify_coverage_with_diff(mut request: CoverageVerification<'_>) {
    if !request.config.coverage.enabled {
        return;
    }
    evaluate_coverage_report(&mut request);
}

fn evaluate_coverage_report(request: &mut CoverageVerification<'_>) {
    let cov_path = request
        .cli_report
        .as_deref()
        .or(request.config.coverage.report.as_deref());
    let Some(ref path_str) = cov_path else {
        record_evidence_failure(
            request.report,
            request.config.gate.strict,
            EvidenceFailure {
                step: "coverage-report",
                target: Path::new("<not-configured>"),
                message: "Coverage is enabled, but no report path was provided.".to_string(),
            },
        );
        return;
    };
    let p = Path::new(path_str);
    if !p.exists() {
        record_evidence_failure(
            request.report,
            request.config.gate.strict,
            EvidenceFailure {
                step: "coverage-report",
                target: p,
                message: "Required coverage report was not found.".to_string(),
            },
        );
        return;
    }
    let scorer = CoverageScorer::new(&request.config.coverage);
    match scorer.parse_lcov(p) {
        Ok(cov_map) => append_coverage_violations(request, &scorer, &cov_map),
        Err(e) => {
            record_evidence_failure(
                request.report,
                request.config.gate.strict,
                EvidenceFailure {
                    step: "coverage-report",
                    target: p,
                    message: format!("Failed to parse required coverage report: {e}"),
                },
            );
        }
    }
}

fn append_coverage_violations(
    request: &mut CoverageVerification<'_>,
    scorer: &CoverageScorer,
    coverage_map: &std::collections::HashMap<PathBuf, crate::engines::coverage::FileCoverage>,
) {
    let violations = match request.changed_lines {
        Some(lines) => scorer.evaluate_diff_coverage(coverage_map, lines),
        None => scorer.evaluate(coverage_map, request.functions, Path::new(".")),
    };
    request.report.coverage_violations.extend(violations);
}

/// Ingest mutation reports (Stryker, cargo-mutants, generic) and flag scores
/// below the configured floor.
pub fn verify_mutation(
    config: &HardgateConfig,
    cli_report: Option<String>,
    report: &mut GateReport,
) {
    if !config.mutation.enabled {
        return;
    }
    let mut_reports = cli_report
        .map(|r| vec![r])
        .or_else(|| config.mutation.reports.clone());

    let Some(reports) = mut_reports else {
        record_evidence_failure(
            report,
            config.gate.strict,
            EvidenceFailure {
                step: "mutation-report",
                target: Path::new("<not-configured>"),
                message: "Mutation is enabled, but no report path was provided.".to_string(),
            },
        );
        return;
    };
    if reports.is_empty() {
        record_evidence_failure(
            report,
            config.gate.strict,
            EvidenceFailure {
                step: "mutation-report",
                target: Path::new("<empty-report-list>"),
                message: "Mutation is enabled, but the configured report list is empty."
                    .to_string(),
            },
        );
        return;
    }
    let gatekeeper = MutationGatekeeper::new(&config.mutation);
    for r_str in reports {
        let p = Path::new(&r_str);
        if !p.exists() {
            record_evidence_failure(
                report,
                config.gate.strict,
                EvidenceFailure {
                    step: "mutation-report",
                    target: p,
                    message: "Required mutation report was not found.".to_string(),
                },
            );
            continue;
        }
        match gatekeeper.evaluate_report(p) {
            Ok(m_violations) => report.mutation_violations.extend(m_violations),
            Err(e) => {
                record_evidence_failure(
                    report,
                    config.gate.strict,
                    EvidenceFailure {
                        step: "mutation-report",
                        target: p,
                        message: format!("Failed to parse required mutation report: {e}"),
                    },
                );
            }
        }
    }
}
