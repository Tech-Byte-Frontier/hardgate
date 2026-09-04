use super::check::{Emission, OutputOptions, emit_gate_report};
use super::dead_code::run_dead_code_analysis;
use super::evidence::{EvidenceFailure, record_evidence_failure};
use super::gate_evidence::{
    GateRun, empty_discovery_advisory, run_generated_freshness, run_legacy_ratchet,
    run_static_gate_or_empty,
};
use super::role_policy::classify_file;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::FileRole;
use crate::engines::coverage::{CoverageEvaluationScope, normalized_repository_key};
use crate::engines::{CoverageScorer, FunctionMetrics, MutationGatekeeper};
use crate::git_evidence::ChangedLineMap;
use anyhow::Result;
use std::collections::BTreeSet;
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
    let root = Path::new(".");
    let config = HardgateConfig::load_or_default(None)?;
    let scoped = !opts.paths.is_empty();

    let GateRun {
        mut report,
        files,
        empty,
        read_results,
        functions,
        ..
    } = run_static_gate_or_empty(&config, false, &opts.paths)?;
    if empty {
        report
            .advisories
            .push(empty_discovery_advisory(false, scoped));
    }

    if config.analysis.dead_code.enabled {
        run_dead_code_analysis(&config, &read_results, root, &mut report)?;
    }

    run_legacy_ratchet(
        &config,
        root,
        &mut report,
        config.analysis.dead_code.enabled,
    );
    run_generated_freshness(&config, root, &mut report);

    let source_files = if config.coverage.enabled {
        source_files_for_coverage(SourceCoverageRequest {
            files: &files,
            functions: &functions,
            root,
            config: &config,
            report: &mut report,
        })
    } else {
        Vec::new()
    };
    verify_coverage_with_scope(
        CoverageVerification {
            config: &config,
            cli_report: opts.coverage_report.clone(),
            functions: &functions,
            changed_lines: None,
            report: &mut report,
        },
        CoverageScope {
            source_files: &source_files,
            root,
        },
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
    )?;
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

/// Current Source-role inventory used to keep report scoring production-only.
pub struct CoverageScope<'a> {
    pub source_files: &'a [PathBuf],
    pub root: &'a Path,
}

/// Ingest an lcov report and flag functions breaching coverage/CRAP floors.
/// Enabled coverage is required evidence regardless of static gate strictness.
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
    evaluate_coverage_report(&mut request, None);
}

/// Internal root/inventory-aware coverage verification used by `check` and
/// `verify`. The compatibility wrappers above intentionally remain unchanged.
pub fn verify_coverage_with_scope(mut request: CoverageVerification<'_>, scope: CoverageScope<'_>) {
    if !request.config.coverage.enabled {
        return;
    }
    evaluate_coverage_report(&mut request, Some(scope));
}

fn evaluate_coverage_report(
    request: &mut CoverageVerification<'_>,
    scope: Option<CoverageScope<'_>>,
) {
    let cov_path = request
        .cli_report
        .as_deref()
        .or(request.config.coverage.report.as_deref());
    let Some(ref path_str) = cov_path else {
        record_evidence_failure(
            request.report,
            true,
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
            true,
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
        Ok(cov_map) => append_coverage_violations(request, &scorer, &cov_map, scope),
        Err(e) => {
            record_evidence_failure(
                request.report,
                true,
                EvidenceFailure {
                    step: "coverage-report",
                    target: p,
                    message: format!("Failed to parse required coverage report: {e:#}"),
                },
            );
        }
    }
}

fn append_coverage_violations(
    request: &mut CoverageVerification<'_>,
    scorer: &CoverageScorer,
    coverage_map: &std::collections::HashMap<PathBuf, crate::engines::coverage::FileCoverage>,
    scope: Option<CoverageScope<'_>>,
) {
    let violations = coverage_violations(request, scorer, coverage_map, scope);
    request.report.coverage_violations.extend(violations);
}

fn coverage_violations(
    request: &CoverageVerification<'_>,
    scorer: &CoverageScorer,
    coverage_map: &std::collections::HashMap<PathBuf, crate::engines::coverage::FileCoverage>,
    scope: Option<CoverageScope<'_>>,
) -> Vec<crate::engines::coverage::CoverageViolation> {
    if let Some(lines) = request.changed_lines {
        return scorer.evaluate_diff_coverage_strict(
            coverage_map,
            lines,
            scope.as_ref().map_or(Path::new("."), |scope| scope.root),
        );
    }
    match scope {
        Some(scope) => scorer.evaluate_for_sources(
            coverage_map,
            request.functions,
            CoverageEvaluationScope {
                root: scope.root,
                source_files: Some(scope.source_files),
            },
        ),
        None => scorer.evaluate(coverage_map, request.functions, Path::new(".")),
    }
}

pub(crate) struct SourceCoverageRequest<'a> {
    pub files: &'a [PathBuf],
    pub functions: &'a [FunctionMetrics],
    pub root: &'a Path,
    pub config: &'a HardgateConfig,
    pub report: &'a mut GateReport,
}

pub(crate) fn source_files_for_coverage(request: SourceCoverageRequest<'_>) -> Vec<PathBuf> {
    // Rust module/re-export files may be valid inventory sources without any
    // executable mapping. Every non-Rust source remains required because its
    // provider can expose executable lines without Hardgate function metrics.
    let executable_rust_files: BTreeSet<String> = request
        .functions
        .iter()
        .filter_map(|function| normalized_repository_key(&function.file, request.root))
        .collect();
    let rust_scope = RustCoverageScope {
        root: request.root,
        executable_files: &executable_rust_files,
    };
    request
        .files
        .iter()
        .filter_map(|path| {
            source_file_for_coverage(path, &rust_scope, request.config, request.report)
        })
        .collect()
}

struct RustCoverageScope<'a> {
    root: &'a Path,
    executable_files: &'a BTreeSet<String>,
}

fn source_file_for_coverage(
    path: &Path,
    rust_scope: &RustCoverageScope<'_>,
    config: &HardgateConfig,
    report: &mut GateReport,
) -> Option<PathBuf> {
    let classified = match classify_file(path, config) {
        Ok(classified) => classified,
        Err(error) => {
            record_evidence_failure(
                report,
                true,
                EvidenceFailure {
                    step: "classify-source",
                    target: path,
                    message: format!("Unable to classify source for coverage: {error}"),
                },
            );
            return None;
        }
    };
    if classified.role != FileRole::Source {
        return None;
    }
    if is_rust_source(path) {
        let Some(key) = normalized_repository_key(path, rust_scope.root) else {
            return None;
        };
        if !rust_scope.executable_files.contains(&key) {
            return None;
        }
    }
    Some(path.to_path_buf())
}

fn is_rust_source(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;

/// Ingest mutation reports (Stryker, cargo-mutants, generic) and flag scores
/// below the configured floor. Enabled mutation is required evidence
/// regardless of static gate strictness.
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
            true,
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
            true,
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
                true,
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
                    true,
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
