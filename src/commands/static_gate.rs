use super::evidence::{EvidenceFailure, record_evidence_failure};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{
    ClassifiedFile, DiscoverOptions, FileRole, discover_files_with_exclusions,
    filter_files_by_paths,
};
use crate::engines::{
    AntiGamingScanner, BudgetViolation, CloneDetector, ComplexityAnalyzer, ComplexityViolation,
    FunctionMetrics, InvariantViolation, InvariantsChecker, SuppressionViolation,
    check_content_budgets,
};
use anyhow::Result;
use rayon::prelude::*;
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
        .map(|(path, content)| (ClassifiedFile::new(path), content.clone()))
        .collect();
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
    let files = filter_files_by_paths(discovery.files, paths, root)?;
    if files.is_empty() {
        return Ok(None);
    }

    let mut report = GateReport::new(config.gate.name.clone());
    record_budget_exclusion_advisory(&discovery.excluded_files, &mut report);
    let classified: Vec<ClassifiedFile> =
        files.iter().map(|path| ClassifiedFile::new(path)).collect();
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

struct CloneRun<'a> {
    read_results: &'a [(PathBuf, String)],
    changed_files: &'a [PathBuf],
    config: &'a HardgateConfig,
    root: &'a Path,
    diff: bool,
}

fn run_clone_analysis(input: CloneRun<'_>, report: &mut GateReport) -> Result<()> {
    if !input.config.clones.enabled {
        return Ok(());
    }
    let detector = CloneDetector::new(&input.config.clones);
    let clone_inputs = if input.diff {
        full_clone_inputs(input.config, input.root, report)?
    } else {
        clone_eligible_inputs(input.read_results)
    };
    record_clone_exclusion_advisory(&detector, &clone_inputs, input.root, report);
    if clone_inputs.len() < 2 {
        return Ok(());
    }
    let mut violations = detector.detect_clones(&clone_inputs, input.root);
    if input.diff {
        violations
            .retain(|violation| clone_touches_files(violation, input.changed_files, input.root));
    }
    report.clone_violations.extend(violations);
    Ok(())
}

fn record_clone_exclusion_advisory(
    detector: &CloneDetector,
    inputs: &[(PathBuf, String)],
    root: &Path,
    report: &mut GateReport,
) {
    let count = detector.count_excluded_files(inputs, root);
    if count == 0 {
        return;
    }
    let noun = if count == 1 { "file" } else { "files" };
    report.advisories.push(format!(
        "{} {} excluded from clone detection via hardgate.toml.",
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
            Err(error) => record_evidence_failure(
                report,
                config.gate.strict,
                EvidenceFailure {
                    step: "read-source",
                    target: &file.path,
                    message: format!("Unable to read classified file: {error}"),
                },
            ),
        }
    }
    (read_results, analyzed_inputs)
}

type FileAnalysis = (
    Vec<BudgetViolation>,
    Vec<SuppressionViolation>,
    Vec<InvariantViolation>,
    Vec<FunctionMetrics>,
    Vec<ComplexityViolation>,
);

struct FileAnalysisContext<'a> {
    config: &'a HardgateConfig,
    root: &'a Path,
    anti_gaming: &'a AntiGamingScanner,
    invariants: &'a InvariantsChecker,
}

fn analyze_inputs(
    inputs: &[(ClassifiedFile, String)],
    context: &FileAnalysisContext<'_>,
) -> Vec<(FileAnalysis, Option<(PathBuf, String)>)> {
    inputs
        .par_iter()
        .map(|(file, content)| analyze_one(file, content, context))
        .collect()
}

fn analyze_one(
    file: &ClassifiedFile,
    content: &str,
    context: &FileAnalysisContext<'_>,
) -> (FileAnalysis, Option<(PathBuf, String)>) {
    let (budgets, suppressions, invariants) = analyze_safety(file, content, context);
    let (functions, violations, parse_error) = analyze_complexity(file, content, context);
    (
        (budgets, suppressions, invariants, functions, violations),
        parse_error,
    )
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
        check_content_budgets(path, content, &context.config.budgets.files, context.root)
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
    let violations =
        ComplexityAnalyzer::check_violations(&functions, &context.config.budgets.functions);
    (functions, violations, None)
}

fn merge_file_analysis(
    analyzed: Vec<(FileAnalysis, Option<(PathBuf, String)>)>,
    config: &HardgateConfig,
    report: &mut GateReport,
) -> Vec<FunctionMetrics> {
    let mut all_functions = Vec::new();
    for ((budgets, suppressions, invariants, functions, violations), parse_error) in analyzed {
        report.budget_violations.extend(budgets);
        report.suppression_violations.extend(suppressions);
        report.invariant_violations.extend(invariants);
        report.complexity_violations.extend(violations);
        all_functions.extend(functions);
        if let Some((path, error)) = parse_error {
            record_evidence_failure(
                report,
                config.gate.strict,
                EvidenceFailure {
                    step: "parse-source",
                    target: &path,
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
        record_evidence_failure(
            report,
            config.gate.strict,
            EvidenceFailure {
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

fn clone_eligible_inputs(read_results: &[(PathBuf, String)]) -> Vec<(PathBuf, String)> {
    read_results
        .iter()
        .filter(|(path, _)| ClassifiedFile::new(path).role.receives_clone_analysis())
        .cloned()
        .collect()
}

fn full_clone_inputs(
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) -> Result<Vec<(PathBuf, String)>> {
    let discovery = discover_files_with_exclusions(DiscoverOptions {
        root,
        diff_only: false,
        exclusions: &config.budgets.files.exclusions.paths,
    })?;
    let files: Vec<ClassifiedFile> = discovery
        .files
        .iter()
        .map(|path| ClassifiedFile::new(path))
        .filter(|file| file.role.receives_clone_analysis())
        .collect();
    Ok(read_files_only(&files, config, report))
}

fn read_files_only(
    files: &[ClassifiedFile],
    config: &HardgateConfig,
    report: &mut GateReport,
) -> Vec<(PathBuf, String)> {
    let mut read = Vec::new();
    for file in files {
        match fs::read_to_string(&file.path) {
            Ok(content) => read.push((file.path.clone(), content)),
            Err(error) => record_evidence_failure(
                report,
                config.gate.strict,
                EvidenceFailure {
                    step: "read-clone-index",
                    target: &file.path,
                    message: format!("Unable to read file required by full clone index: {error}"),
                },
            ),
        }
    }
    read
}

fn clone_touches_files(
    violation: &crate::engines::CloneViolation,
    files: &[PathBuf],
    root: &Path,
) -> bool {
    files.iter().any(|path| {
        let rel = path.strip_prefix(root).unwrap_or(path);
        rel == violation.file_a || rel == violation.file_b
    })
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
    let classified = ClassifiedFile::new(input.path);
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
