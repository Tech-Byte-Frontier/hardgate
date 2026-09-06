use super::evidence::{EvidenceFailure, record_evidence_failure};
use super::role_policy::classify_files;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::FileRole;
use crate::engines::coverage::{CoverageEvaluationScope, normalized_repository_key};
use crate::engines::{CoverageScorer, FunctionMetrics, MutationGatekeeper};
use crate::git_evidence::ChangedLineMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

/// Ingest an lcov report and flag functions breaching coverage floors.
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

/// Root/inventory-aware coverage verification used by `check`. Low-level
/// scoring wrappers above do not establish source-bound acceptance.
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
    let resolved = scope.as_ref().map_or_else(
        || PathBuf::from(path_str),
        |scope| scope.root.join(path_str),
    );
    let p = resolved.as_path();
    if !p.exists() {
        record_evidence_failure(
            request.report,
            true,
            EvidenceFailure {
                step: "coverage-report",
                target: Path::new(path_str),
                message: "Required coverage report was not found.".to_string(),
            },
        );
        return;
    }
    let scorer = CoverageScorer::new(&request.config.coverage);
    if let Some(ref scope) = scope
        && let Err(error) = crate::evidence::verify(
            scope.root,
            p,
            crate::evidence::EvidenceKind::Coverage,
            request.config,
        )
    {
        record_evidence_failure(
            request.report,
            true,
            EvidenceFailure {
                step: "coverage-report",
                target: p,
                message: format!("Required coverage source identity is invalid: {error:#}"),
            },
        );
        return;
    }
    let parsed = match scope.as_ref() {
        Some(scope) => scorer.parse_lcov_for_project(p, scope.root, request.config),
        None => scorer.parse_lcov(p),
    };
    match parsed {
        Ok(cov_map) => append_coverage_violations(request, &scorer, &cov_map, scope),
        Err(e) => {
            record_evidence_failure(
                request.report,
                true,
                EvidenceFailure {
                    step: "coverage-report",
                    target: Path::new(path_str),
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
    if coverage_has_inputs(request, coverage_map, scope.as_ref()) {
        request.report.observe_engine(
            crate::diagnostics::execution::EngineId::Coverage,
            crate::diagnostics::execution::EngineState::Completed,
        );
    }
    let violations = coverage_violations(request, scorer, coverage_map, scope);
    request.report.coverage_violations.extend(violations);
}

fn coverage_has_inputs(
    request: &CoverageVerification<'_>,
    coverage_map: &std::collections::HashMap<PathBuf, crate::engines::coverage::FileCoverage>,
    scope: Option<&CoverageScope<'_>>,
) -> bool {
    if let Some(lines) = request.changed_lines {
        return lines.values().any(|lines| !lines.is_empty());
    }
    scope.map_or(!coverage_map.is_empty(), |scope| {
        !scope.source_files.is_empty()
            || request
                .config
                .coverage
                .critical_paths
                .as_ref()
                .is_some_and(|paths| !paths.is_empty())
    })
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
        .filter(|function| !function.test_only)
        .filter_map(|function| normalized_repository_key(&function.file, request.root))
        .collect();
    let rust_scope = RustCoverageScope {
        root: request.root,
        executable_files: &executable_rust_files,
    };
    let classified = match classify_files(request.files, request.config, request.root) {
        Ok(files) => files,
        Err(error) => {
            record_evidence_failure(
                request.report,
                true,
                EvidenceFailure {
                    step: "coverage-source-classification",
                    target: request.root,
                    message: format!("Unable to classify source for coverage: {error}"),
                },
            );
            return Vec::new();
        }
    };
    classified
        .iter()
        .filter_map(|file| source_file_for_coverage(file, &rust_scope))
        .collect()
}

struct RustCoverageScope<'a> {
    root: &'a Path,
    executable_files: &'a BTreeSet<String>,
}

fn source_file_for_coverage(
    classified: &crate::discovery::ClassifiedFile,
    rust_scope: &RustCoverageScope<'_>,
) -> Option<PathBuf> {
    let path = classified.path.as_path();
    if classified.role != FileRole::Source {
        return None;
    }
    if is_rust_source(path) {
        let key = normalized_repository_key(path, rust_scope.root)?;
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
    evaluate_mutation_reports(
        config,
        cli_report,
        report,
        MutationScope {
            root: Path::new("."),
            bind_source: false,
        },
    );
}

pub fn verify_mutation_at(
    config: &HardgateConfig,
    cli_report: Option<String>,
    report: &mut GateReport,
    root: &Path,
) {
    evaluate_mutation_reports(
        config,
        cli_report,
        report,
        MutationScope {
            root,
            bind_source: true,
        },
    );
}

struct MutationScope<'a> {
    root: &'a Path,
    bind_source: bool,
}

fn evaluate_mutation_reports(
    config: &HardgateConfig,
    cli_report: Option<String>,
    report: &mut GateReport,
    scope: MutationScope<'_>,
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
    for path in reports {
        evaluate_mutation_report(config, &path, report, &scope);
    }
}

fn evaluate_mutation_report(
    config: &HardgateConfig,
    path: &str,
    report: &mut GateReport,
    scope: &MutationScope<'_>,
) {
    let resolved = scope.root.join(path);
    let result = validate_mutation_report(config, &resolved, scope);
    match result {
        Ok(violations) => {
            report.observe_engine(
                crate::diagnostics::execution::EngineId::MutationReport,
                crate::diagnostics::execution::EngineState::Completed,
            );
            report.mutation_violations.extend(violations);
        }
        Err(message) => record_evidence_failure(
            report,
            true,
            EvidenceFailure {
                step: "mutation-report",
                target: &resolved,
                message,
            },
        ),
    }
}

fn validate_mutation_report(
    config: &HardgateConfig,
    path: &Path,
    scope: &MutationScope<'_>,
) -> Result<Vec<crate::engines::mutation::MutationViolation>, String> {
    if !path.exists() {
        return Err("Required mutation report was not found.".to_string());
    }
    if scope.bind_source {
        crate::evidence::verify(
            scope.root,
            path,
            crate::evidence::EvidenceKind::Mutation,
            config,
        )
        .map_err(|error| format!("Required mutation source identity is invalid: {error:#}"))?;
    }
    MutationGatekeeper::new(&config.mutation)
        .evaluate_report(path)
        .map_err(|error| format!("Failed to parse required mutation report: {error}"))
}
