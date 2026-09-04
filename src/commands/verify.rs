use super::check::{Emission, OutputOptions, emit_gate_report, print_empty_discovery};
use super::evidence::record_evidence_failure;
use super::static_gate::run_static_gate_scoped;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::engines::{CoverageScorer, FunctionMetrics, MutationGatekeeper};
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

/// Ingest an lcov report and flag functions breaching coverage/CRAP floors.
pub fn verify_coverage(
    config: &HardgateConfig,
    cli_report: Option<String>,
    functions: &[FunctionMetrics],
    report: &mut GateReport,
) {
    if !config.coverage.enabled {
        return;
    }
    let cov_path = cli_report.or_else(|| config.coverage.report.clone());
    let Some(ref path_str) = cov_path else {
        record_evidence_failure(
            report,
            config.gate.strict,
            "coverage-report",
            Path::new("<not-configured>"),
            "Coverage is enabled, but no report path was provided.".to_string(),
        );
        return;
    };
    let p = Path::new(path_str);
    if !p.exists() {
        record_evidence_failure(
            report,
            config.gate.strict,
            "coverage-report",
            p,
            "Required coverage report was not found.".to_string(),
        );
        return;
    }
    let scorer = CoverageScorer::new(&config.coverage);
    match scorer.parse_lcov(p) {
        Ok(cov_map) => {
            let cov_violations = scorer.evaluate(&cov_map, functions, Path::new("."));
            report.coverage_violations.extend(cov_violations);
        }
        Err(e) => {
            record_evidence_failure(
                report,
                config.gate.strict,
                "coverage-report",
                p,
                format!("Failed to parse required coverage report: {e}"),
            );
        }
    }
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
            "mutation-report",
            Path::new("<not-configured>"),
            "Mutation is enabled, but no report path was provided.".to_string(),
        );
        return;
    };
    if reports.is_empty() {
        record_evidence_failure(
            report,
            config.gate.strict,
            "mutation-report",
            Path::new("<empty-report-list>"),
            "Mutation is enabled, but the configured report list is empty.".to_string(),
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
                "mutation-report",
                p,
                "Required mutation report was not found.".to_string(),
            );
            continue;
        }
        match gatekeeper.evaluate_report(p) {
            Ok(m_violations) => report.mutation_violations.extend(m_violations),
            Err(e) => {
                record_evidence_failure(
                    report,
                    config.gate.strict,
                    "mutation-report",
                    p,
                    format!("Failed to parse required mutation report: {e}"),
                );
            }
        }
    }
}
