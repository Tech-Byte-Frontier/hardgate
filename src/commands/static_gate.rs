use super::evidence::{EvidenceFailure, record_evidence_failure};
use super::role_policy::{
    CloneRun, RoleEvidence, apply_budget_findings, apply_complexity_findings,
    apply_invariant_findings, apply_suppression_findings, classify_file, classify_files,
    effective_file_budgets, effective_function_budgets, record_role_evidence_failure,
    run_clone_analysis,
};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{
    ClassifiedFile, DiscoverOptions, FileRole, discover_files_with_exclusions,
    filter_files_by_paths,
};
use crate::engines::{
    AntiGamingScanner, BudgetViolation, ComplexityAnalyzer, ComplexityViolation, FunctionMetrics,
    InvariantViolation, InvariantsChecker, SuppressionViolation, check_content_budgets,
};
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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
    let files: Vec<PathBuf> = contents.iter().map(|(path, _)| path.clone()).collect();
    let classified: Vec<(ClassifiedFile, String)> = contents
        .iter()
        .map(|(path, content)| Ok((classify_file(path, config)?, content.clone())))
        .collect::<Result<_>>()?;
    let mut report = GateReport::new(config.gate.name.clone());
    let roles: Vec<ClassifiedFile> = classified.iter().map(|(file, _)| file.clone()).collect();
    record_classification_gaps(&roles, config, root, &mut report);
    let functions = analyze_loaded_files(&classified, config, root, &mut report);
    let read_results = contents.to_vec();
    run_clone_analysis(
        CloneRun {
            read_results: &read_results,
            changed_files: &[],
            config,
            root,
            diff: false,
        },
        &mut report,
    )?;
    Ok((report, files, read_results, functions))
}

/// Run the static gate, optionally scoped to explicit files or directories.
pub fn run_static_gate_scoped(
    config: &HardgateConfig,
    diff: bool,
    paths: &[PathBuf],
) -> Result<StaticGateOutcome> {
    let root = Path::new(".");
    let discovery = discover_files_with_exclusions(DiscoverOptions {
        root,
        diff_only: diff,
        exclusions: &config.budgets.files.exclusions.paths,
    })?;
    let (files, excluded_files) = select_files(config, diff, paths, discovery)?;
    if files.is_empty() {
        return Ok(None);
    }

    let mut report = GateReport::new(config.gate.name.clone());
    record_budget_exclusion_advisory(&excluded_files, &mut report);
    let classified = classify_files(&files, config)?;
    record_classification_gaps(&classified, config, root, &mut report);
    let (read_results, all_functions) = run_file_analysis(&classified, config, root, &mut report);
    run_clone_analysis(
        CloneRun {
            read_results: &read_results,
            changed_files: &files,
            config,
            root,
            diff,
        },
        &mut report,
    )?;
    Ok(Some((report, files, read_results, all_functions)))
}

fn select_files(
    config: &HardgateConfig,
    diff: bool,
    paths: &[PathBuf],
    discovery: crate::discovery::DiscoveryResult,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let root = Path::new(".");
    let scope_paths = normalize_scope_paths(paths, root)?;
    let crate::discovery::DiscoveryResult {
        files: discovered_files,
        excluded_files: discovered_excluded,
        ..
    } = discovery;

    let (mut files, mut excluded_files) = if diff && !paths.is_empty() {
        let full_discovery = discover_files_with_exclusions(DiscoverOptions {
            root,
            diff_only: false,
            exclusions: &config.budgets.files.exclusions.paths,
        })?;
        let explicit_files = filter_files_by_paths(full_discovery.files, &scope_paths, root)?;
        let mut files = discovered_files;
        files.extend(explicit_files);
        let mut excluded_files = discovered_excluded;
        excluded_files.extend(full_discovery.excluded_files);
        (files, excluded_files)
    } else {
        (
            filter_files_by_paths(discovered_files, &scope_paths, root)?,
            discovered_excluded,
        )
    };

    files.sort();
    files.dedup();
    excluded_files.sort();
    excluded_files.dedup();

    // Discovery intentionally keeps budget-excluded files in `files`; only
    // report an advisory for excluded files that survived the selected scope.
    // This also removes duplicates when diff and full discoveries overlap.
    let selected: HashSet<String> = files.iter().map(|path| path_key(path)).collect();
    excluded_files.retain(|path| selected.contains(&path_key(path)));

    Ok((files, excluded_files))
}

fn normalize_scope_paths(paths: &[PathBuf], root: &Path) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let absolute_root = fs::canonicalize(root)?;
    paths
        .iter()
        .map(|path| {
            let absolute_path = if path.is_absolute() {
                path.clone()
            } else {
                absolute_root.join(path)
            };
            if !absolute_path.exists() {
                anyhow::bail!("Path not found: {}", path.display());
            }
            let absolute_path = fs::canonicalize(absolute_path)?;
            Ok(absolute_path
                .strip_prefix(&absolute_root)
                .map(PathBuf::from)
                .unwrap_or(absolute_path))
        })
        .collect()
}

fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    value.strip_prefix("./").unwrap_or(&value).to_string()
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

fn run_file_analysis(
    files: &[ClassifiedFile],
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) -> (Vec<(PathBuf, String)>, Vec<FunctionMetrics>) {
    let (read_results, analyzed_inputs) = read_classified_files(files, config, report);
    let functions = analyze_loaded_files(&analyzed_inputs, config, root, report);
    (read_results, functions)
}

fn analyze_loaded_files(
    analyzed_inputs: &[(ClassifiedFile, String)],
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
    merge_file_analysis(analyzed, config, report)
}

type ReadInputs = (Vec<(PathBuf, String)>, Vec<(ClassifiedFile, String)>);

fn read_classified_files(
    files: &[ClassifiedFile],
    config: &HardgateConfig,
    report: &mut GateReport,
) -> ReadInputs {
    let attempts: Vec<(ClassifiedFile, std::result::Result<String, String>)> = files
        .par_iter()
        .map(|file| {
            let result = fs::read_to_string(&file.path).map_err(|error| error.to_string());
            (file.clone(), result)
        })
        .collect();
    let mut read_results = Vec::new();
    let mut analyzed_inputs = Vec::new();
    for (file, result) in attempts {
        match result {
            Ok(content) => {
                read_results.push((file.path.clone(), content.clone()));
                analyzed_inputs.push((file, content));
            }
            Err(error) => record_role_evidence_failure(
                report,
                RoleEvidence {
                    config,
                    role: file.role,
                    step: "read-source",
                    target: &file.path,
                    message: format!("Unable to read classified file: {error}"),
                },
            ),
        }
    }
    (read_results, analyzed_inputs)
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
    inputs: &[(ClassifiedFile, String)],
    context: &FileAnalysisContext<'_>,
) -> Vec<FileAnalysis> {
    inputs
        .par_iter()
        .map(|(file, content)| analyze_one(file, content, context))
        .collect()
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

fn record_classification_gaps(
    files: &[ClassifiedFile],
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) {
    let generated = files
        .iter()
        .filter(|file| file.role == FileRole::Generated)
        .count();
    if generated > 0 {
        report.advisories.push(format!(
            "Classified {generated} generated file(s); inventoried without handwritten complexity or clone debt."
        ));
    }
    for file in files {
        record_classification_gap(file, config, root, report);
    }
}

fn record_classification_gap(
    file: &ClassifiedFile,
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) {
    let rel = file.path.strip_prefix(root).unwrap_or(&file.path);
    if file.role == FileRole::Unknown && config.gate.enforce_classified_sources {
        record_evidence_failure(
            report,
            true,
            EvidenceFailure {
                step: "classify-source",
                target: rel,
                message: "No repository role matched this file.".to_string(),
            },
        );
    } else if matches!(file.role, FileRole::Source | FileRole::Migration) && !file.ast_supported {
        record_role_evidence_failure(
            report,
            RoleEvidence {
                config,
                role: file.role,
                step: "unsupported-source",
                target: rel,
                message: format!(
                    "File is classified as {:?}, but no AST engine supports its extension.",
                    file.role
                ),
            },
        );
    }
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
    let classified = match classify_file(input.path, input.config) {
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
    record_classification_gaps(
        std::slice::from_ref(&classified),
        input.config,
        input.root,
        report,
    );
    let context = FileAnalysisContext {
        config: input.config,
        root: input.root,
        anti_gaming: input.anti_gaming,
        invariants: input.invariants,
    };
    let analyzed = analyze_one(&classified, input.content, &context);
    merge_file_analysis(vec![analyzed], input.config, report)
}
