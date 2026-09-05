mod classification_gaps;
use super::dead_code::{DeadCodeScope, run_scoped_dead_code_analysis};
use classification_gaps::record_classification_gaps;
mod excerpts;
mod observations;
mod selection;
#[cfg(test)]
mod snapshot_tests;

use super::evidence::{EvidenceFailure, record_evidence_failure};
use super::role_policy::{
    CloneRun, RoleEvidence, apply_budget_findings, apply_complexity_findings,
    apply_invariant_findings, apply_suppression_findings, classify_file, classify_files,
    effective_file_budgets, effective_function_budgets, record_role_evidence_failure,
    run_clone_analysis,
};
use super::source_snapshot::{SharedSource, SourceSnapshot};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{ClassifiedFile, DiscoverOptions, FileRole, discover_paths};
use crate::engines::{
    AntiGamingScanner, BudgetViolation, ComplexityAnalyzer, ComplexityViolation, FunctionMetrics,
    InvariantViolation, InvariantsChecker, SuppressionViolation, check_content_budgets,
};
use anyhow::Result;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Artifacts of one static-gate run: the report plus the discovered files,
/// their contents, and per-function metrics for downstream gates.
pub type StaticGateOutcome = Option<(
    GateReport,
    Vec<PathBuf>,
    Vec<(PathBuf, String)>,
    Vec<FunctionMetrics>,
)>;

/// Static-gate artifacts computed directly from a Git snapshot's contents.
pub type StaticSnapshotOutcome = (
    GateReport,
    Vec<PathBuf>,
    Vec<(PathBuf, String)>,
    Vec<FunctionMetrics>,
);

/// Run the static gate over the whole discovered tree.
pub fn run_static_gate(config: &HardgateConfig, diff: bool) -> Result<StaticGateOutcome> {
    run_static_gate_scoped(config, diff, &[])
}

pub fn run_static_gate_snapshot(
    config: &HardgateConfig,
    contents: &[(PathBuf, String)],
) -> Result<StaticSnapshotOutcome> {
    let root = Path::new(".");
    let files = contents
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let classified = classify_files(&files, config, root)?;
    let snapshot = SourceSnapshot::from_shared(
        classified
            .into_iter()
            .zip(contents)
            .map(|(file, (_, text))| (file, Arc::from(text.as_str())))
            .collect(),
    );
    let request = StaticRequest {
        config,
        root,
        paths: &[],
        diff: false,
        dead_code: false,
        snippets: false,
    };
    let outcome = analyze_snapshot(request, files, Vec::new(), snapshot)?;
    Ok((
        outcome.report,
        outcome.files,
        contents.to_vec(),
        outcome.functions,
    ))
}

pub(crate) struct StaticRequest<'a> {
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub paths: &'a [PathBuf],
    pub diff: bool,
    pub dead_code: bool,
    pub snippets: bool,
}

pub(crate) struct StaticAnalysis {
    pub report: GateReport,
    pub files: Vec<PathBuf>,
    pub read_results: Vec<SharedSource>,
    pub functions: Vec<FunctionMetrics>,
    pub empty: bool,
}

/// Run the static gate, optionally scoped to explicit files or directories.
pub fn run_static_gate_scoped(
    config: &HardgateConfig,
    diff: bool,
    paths: &[PathBuf],
) -> Result<StaticGateOutcome> {
    run_static_gate_at(config, diff, paths, Path::new("."))
}

pub fn run_static_gate_at(
    config: &HardgateConfig,
    diff: bool,
    paths: &[PathBuf],
    root: &Path,
) -> Result<StaticGateOutcome> {
    let run = run_shared_gate(StaticRequest {
        config,
        root,
        paths,
        diff,
        dead_code: false,
        snippets: false,
    })?;
    if run.empty {
        return Ok(None);
    }
    let contents = run
        .read_results
        .into_iter()
        .map(|(path, text)| (path, text.to_string()))
        .collect();
    Ok(Some((run.report, run.files, contents, run.functions)))
}

pub(crate) fn run_shared_gate(request: StaticRequest<'_>) -> Result<StaticAnalysis> {
    let reference_context =
        request.dead_code || (request.diff && clone_context_enabled(request.config));
    let full = if reference_context || (request.diff && !request.paths.is_empty()) {
        Some(discover_paths(DiscoverOptions {
            root: request.root,
            diff_only: false,
            exclusions: &request.config.budgets.files.exclusions.paths,
        })?)
    } else {
        None
    };
    let discovery = if !request.diff
        && let Some(full) = &full
    {
        crate::discovery::DiscoveryResult {
            files: full.files.clone(),
            excluded_files: full.excluded_files.clone(),
            classified_files: Vec::new(),
        }
    } else {
        discover_paths(DiscoverOptions {
            root: request.root,
            diff_only: request.diff,
            exclusions: &request.config.budgets.files.exclusions.paths,
        })?
    };
    let (files, excluded) = selection::select_files(
        selection::Scope {
            config: request.config,
            diff: request.diff,
            paths: request.paths,
            root: request.root,
            full: full.as_ref(),
        },
        discovery,
    )?;
    if files.is_empty() {
        return analyze_snapshot(request, files, excluded, SourceSnapshot::default());
    }
    let mut context_paths = if reference_context {
        full.map(|full| full.files).unwrap_or_default()
    } else {
        Vec::new()
    };
    context_paths.extend_from_slice(&files);
    context_paths.sort();
    context_paths.dedup();
    let classified = classify_files(&context_paths, request.config, request.root)?;
    analyze_snapshot(
        request,
        files,
        excluded,
        SourceSnapshot::capture(classified),
    )
}

fn clone_context_enabled(config: &HardgateConfig) -> bool {
    FileRole::POLICY_ROLES.into_iter().any(|role| {
        let override_enabled = config
            .roles
            .for_role(role)
            .and_then(|policy| policy.clone_enabled);
        (role.receives_clone_analysis() || override_enabled == Some(true))
            && override_enabled.unwrap_or(config.clones.enabled)
    })
}

fn analyze_snapshot(
    request: StaticRequest<'_>,
    files: Vec<PathBuf>,
    excluded: Vec<PathBuf>,
    snapshot: SourceSnapshot,
) -> Result<StaticAnalysis> {
    let mut report = GateReport::new(request.config.gate.name.clone());
    record_budget_exclusion_advisory(&excluded, &mut report);
    let selected = files
        .iter()
        .filter_map(|path| snapshot.find(path))
        .collect::<Vec<_>>();
    let classified = selected
        .iter()
        .map(|file| &file.classified)
        .collect::<Vec<_>>();
    record_classification_gaps(&classified, request.config, request.root, &mut report);
    let mut loaded = Vec::new();
    for file in selected {
        match &file.content {
            Ok(text) => loaded.push((&file.classified, text.as_ref())),
            Err(error) => record_role_evidence_failure(
                &mut report,
                RoleEvidence {
                    config: request.config,
                    role: file.classified.role,
                    step: "read-source",
                    target: &file.classified.path,
                    message: format!("Unable to read classified file: {error}"),
                },
            ),
        }
    }
    let functions = analyze_loaded_files(&loaded, request.config, request.root, &mut report);
    run_clone_analysis(
        CloneRun {
            snapshot: &snapshot,
            selected_ids: &snapshot.selected_ids(&files),
            changed_files: &files,
            config: request.config,
            root: request.root,
            diff: request.diff,
        },
        &mut report,
    )?;
    if request.dead_code {
        run_scoped_dead_code_analysis(
            DeadCodeScope {
                config: request.config,
                root: request.root,
                selected: &files,
                snapshot: &snapshot,
            },
            &mut report,
        )?;
    }
    if request.snippets {
        excerpts::capture(&snapshot, request.root, &mut report);
    }
    Ok(StaticAnalysis {
        report,
        read_results: snapshot.shared_contents(&files),
        functions,
        empty: files.is_empty(),
        files,
    })
}

fn record_budget_exclusion_advisory(excluded_files: &[PathBuf], report: &mut GateReport) {
    if excluded_files.is_empty() {
        return;
    }
    let count = excluded_files.len();
    let noun = if count == 1 { "file" } else { "files" };
    report.advisories.push(format!(
        "{} {} excluded from file budget checks via hardgate.toml.",
        count, noun
    ));
}

fn analyze_loaded_files(
    analyzed_inputs: &[(&ClassifiedFile, &str)],
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) -> Vec<FunctionMetrics> {
    let anti_gaming = AntiGamingScanner::new(&config.anti_gaming);
    let invariants = InvariantsChecker::new(&config.invariants.rules);
    let context = FileAnalysisContext {
        config,
        root,
        anti_gaming: &anti_gaming,
        invariants: &invariants,
    };
    let analyzed = analyze_inputs(analyzed_inputs, &context);
    for (file, _) in analyzed_inputs {
        observations::observe_file(file, config, report);
    }
    merge_file_analysis(analyzed, config, report)
}

struct FileAnalysis {
    role: FileRole,
    path: PathBuf,
    budgets: Vec<BudgetViolation>,
    suppressions: Vec<SuppressionViolation>,
    invariants: Vec<InvariantViolation>,
    functions: Vec<FunctionMetrics>,
    complexity: Vec<ComplexityViolation>,
    parse_error: Option<String>,
}

struct FileAnalysisContext<'a> {
    config: &'a HardgateConfig,
    root: &'a Path,
    anti_gaming: &'a AntiGamingScanner,
    invariants: &'a InvariantsChecker,
}

fn analyze_inputs(
    inputs: &[(&ClassifiedFile, &str)],
    context: &FileAnalysisContext<'_>,
) -> Vec<FileAnalysis> {
    if inputs.len() < 8 {
        inputs
            .iter()
            .map(|(file, content)| analyze_one(file, content, context))
            .collect()
    } else {
        inputs
            .par_iter()
            .map(|(file, content)| analyze_one(file, content, context))
            .collect()
    }
}

fn analyze_one(
    file: &ClassifiedFile,
    content: &str,
    context: &FileAnalysisContext<'_>,
) -> FileAnalysis {
    let (budgets, suppressions, invariants) = analyze_safety(file, content, context);
    let (functions, complexity, parse_error) = analyze_complexity(file, content, context);
    FileAnalysis {
        role: file.role,
        path: file.path.clone(),
        budgets,
        suppressions,
        invariants,
        functions,
        complexity,
        parse_error: parse_error.map(|(_, error)| error),
    }
}

fn analyze_safety(
    file: &ClassifiedFile,
    content: &str,
    context: &FileAnalysisContext<'_>,
) -> (
    Vec<BudgetViolation>,
    Vec<SuppressionViolation>,
    Vec<InvariantViolation>,
) {
    let path = &file.path;
    let safety = file.role.receives_safety_checks();
    let budgets = if safety {
        let policy = effective_file_budgets(context.config, file.role);
        check_content_budgets(path, content, &policy, context.root)
    } else {
        Vec::new()
    };
    let suppressions = if safety && context.config.anti_gaming.disallow_suppressions {
        context
            .anti_gaming
            .scan_content(path, content, context.root)
    } else {
        Vec::new()
    };
    let invariants = if receives_invariants(file) && context.config.invariants.enforce {
        context.invariants.check_file(path, content, context.root)
    } else {
        Vec::new()
    };
    (budgets, suppressions, invariants)
}

fn receives_invariants(file: &ClassifiedFile) -> bool {
    matches!(file.role, FileRole::Source | FileRole::Test)
}

fn analyze_complexity(
    file: &ClassifiedFile,
    content: &str,
    context: &FileAnalysisContext<'_>,
) -> (
    Vec<FunctionMetrics>,
    Vec<ComplexityViolation>,
    Option<(PathBuf, String)>,
) {
    if !file.role.receives_complexity() || !file.ast_supported {
        return (Vec::new(), Vec::new(), None);
    }
    let path = &file.path;
    let mut analyzer = ComplexityAnalyzer::new();
    let parsed = analyzer.analyze_file_checked(path, content, context.root);
    let functions = match parsed {
        Ok(functions) => functions,
        Err(error) => {
            return (
                Vec::new(),
                Vec::new(),
                Some((path.clone(), error.to_string())),
            );
        }
    };
    let policy = effective_function_budgets(context.config, file.role);
    let violations = ComplexityAnalyzer::check_violations(&functions, &policy);
    (functions, violations, None)
}

fn merge_file_analysis(
    analyzed: Vec<FileAnalysis>,
    config: &HardgateConfig,
    report: &mut GateReport,
) -> Vec<FunctionMetrics> {
    let mut all_functions = Vec::new();
    for file in analyzed {
        apply_budget_findings(report, config, file.role, file.budgets);
        apply_suppression_findings(report, config, file.role, file.suppressions);
        apply_invariant_findings(report, config, file.role, file.invariants);
        apply_complexity_findings(report, config, file.role, file.complexity);
        all_functions.extend(file.functions);
        if let Some(error) = file.parse_error {
            record_role_evidence_failure(
                report,
                RoleEvidence {
                    config,
                    role: file.role,
                    step: "parse-source",
                    target: &file.path,
                    message: error,
                },
            );
        }
    }
    all_functions
}

/// Shared single-file analysis used by `scan` and the MCP server.
pub struct AnalyzeInput<'a> {
    pub path: &'a Path,
    pub content: &'a str,
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub anti_gaming: &'a AntiGamingScanner,
    pub invariants: &'a InvariantsChecker,
}

pub fn analyze_file_content(input: AnalyzeInput, report: &mut GateReport) -> Vec<FunctionMetrics> {
    let classified = match classify_file(input.path, input.config, input.root) {
        Ok(file) => file,
        Err(error) => {
            record_evidence_failure(
                report,
                true,
                EvidenceFailure {
                    step: "classify-source",
                    target: input.path,
                    message: format!("Unable to classify file: {error}"),
                },
            );
            return Vec::new();
        }
    };
    record_classification_gaps(&[&classified], input.config, input.root, report);
    let context = FileAnalysisContext {
        config: input.config,
        root: input.root,
        anti_gaming: input.anti_gaming,
        invariants: input.invariants,
    };
    let analyzed = analyze_one(&classified, input.content, &context);
    observations::observe_file(&classified, input.config, report);
    merge_file_analysis(vec![analyzed], input.config, report)
}
