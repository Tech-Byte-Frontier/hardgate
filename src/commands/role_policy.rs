use super::evidence::{EvidenceFailure, record_evidence_failure};
use crate::config::{CloneConfig, FileBudgets, FunctionBudgets, HardgateConfig, Severity};
use crate::diagnostics::GateReport;
use crate::discovery::{ClassifiedFile, FileRole};
use crate::engines::{
    BudgetViolation, CloneDetector, CloneViolation, ComplexityViolation, DeadCodeViolation,
    InvariantViolation, SuppressionViolation,
};
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
pub(crate) struct RoleEvidence<'a> {
    pub config: &'a HardgateConfig,
    pub role: FileRole,
    pub step: &'static str,
    pub target: &'a Path,
    pub message: String,
}
pub(crate) struct Advisory<'a> {
    pub role: FileRole,
    pub category: &'a str,
    pub target: &'a Path,
    pub detail: String,
}
struct WarningBatch<T, F> {
    role: FileRole,
    category: &'static str,
    findings: Vec<T>,
    detail: F,
}
pub(crate) fn classify_file(path: &Path, config: &HardgateConfig) -> Result<ClassifiedFile> {
    ClassifiedFile::new_with_config(path, &config.classification)
}
pub(crate) fn classify_files(
    paths: &[PathBuf],
    config: &HardgateConfig,
) -> Result<Vec<ClassifiedFile>> {
    paths
        .iter()
        .map(|path| classify_file(path, config))
        .collect()
}
/// Resolve a role severity, falling back to the legacy gate strictness when
/// the role section omits severity (or has no first-class section).
pub(crate) fn severity(config: &HardgateConfig, role: FileRole) -> Severity {
    config
        .roles
        .for_role(role)
        .and_then(|policy| policy.severity)
        .unwrap_or(if config.gate.strict {
            Severity::Error
        } else {
            Severity::Warning
        })
}
pub(crate) fn effective_file_budgets(config: &HardgateConfig, role: FileRole) -> FileBudgets {
    let mut budgets = config.budgets.files.clone();
    let Some(policy) = config.roles.for_role(role) else {
        return budgets;
    };
    if let Some(max_bytes) = policy.max_bytes {
        budgets.max_bytes = Some(max_bytes);
    }
    if let Some(max_lines) = policy.max_lines {
        // Role max_lines is a scalar ceiling, so it overlays every extension
        // entry as well as the global fallback.
        for value in budgets.max_lines.values_mut() {
            *value = max_lines;
        }
        budgets.max_lines.insert("default".to_string(), max_lines);
    }
    budgets
}
pub(crate) fn effective_function_budgets(
    config: &HardgateConfig,
    role: FileRole,
) -> FunctionBudgets {
    let mut budgets = config.budgets.functions.clone();
    let Some(policy) = config.roles.for_role(role) else {
        return budgets;
    };
    if let Some(value) = policy.max_cyclomatic {
        budgets.max_cyclomatic = Some(value);
    }
    if let Some(value) = policy.max_cognitive {
        budgets.max_cognitive = Some(value);
    }
    if let Some(value) = policy.max_halstead_difficulty {
        budgets.max_halstead_difficulty = Some(value);
    }
    if let Some(value) = policy.max_abc {
        budgets.max_abc = Some(value);
    }
    if let Some(value) = policy.max_parameters {
        budgets.max_parameters = Some(value);
    }
    if let Some(value) = policy.max_function_lines {
        budgets.max_lines = Some(value);
    }
    if let Some(value) = policy.max_statements {
        budgets.max_statements = Some(value);
    }
    if let Some(value) = policy.max_nesting_depth {
        budgets.max_nesting_depth = Some(value);
    }
    budgets
}
pub(crate) fn clone_config_for_role(
    config: &HardgateConfig,
    role: FileRole,
) -> Option<CloneConfig> {
    if !config.clones.enabled {
        return None;
    }
    let mut clone = config.clones.clone();
    let Some(policy) = config.roles.for_role(role) else {
        return Some(clone);
    };
    if policy.clone_enabled == Some(false) {
        return None;
    }
    if let Some(value) = policy.clone_min_lines {
        clone.min_lines = value;
    }
    if let Some(value) = policy.clone_min_tokens {
        clone.min_tokens = value;
    }
    Some(clone)
}
pub(crate) fn record_role_evidence_failure(report: &mut GateReport, failure: RoleEvidence<'_>) {
    match severity(failure.config, failure.role) {
        Severity::Error => record_evidence_failure(
            report,
            true,
            EvidenceFailure {
                step: failure.step,
                target: failure.target,
                message: failure.message,
            },
        ),
        Severity::Warning => push_advisory(
            report,
            Advisory {
                role: failure.role,
                category: failure.step,
                target: failure.target,
                detail: failure.message,
            },
        ),
        Severity::Ignore => {}
    }
}
pub(crate) fn push_advisory(report: &mut GateReport, advisory: Advisory<'_>) {
    report.advisories.push(format!(
        "role {:?} advisory: {} for `{}`: {}",
        advisory.role,
        advisory.category,
        advisory.target.display(),
        advisory.detail
    ));
}
pub(crate) fn apply_budget_findings(
    report: &mut GateReport,
    config: &HardgateConfig,
    role: FileRole,
    findings: Vec<BudgetViolation>,
) {
    match severity(config, role) {
        Severity::Error => report.budget_violations.extend(findings),
        Severity::Warning => apply_warning(
            report,
            WarningBatch {
                role,
                category: "file budget",
                findings,
                detail: |finding: &BudgetViolation| {
                    (
                        finding.file.clone(),
                        format!(
                            "{} (actual {}, limit {})",
                            finding.message, finding.actual, finding.limit
                        ),
                    )
                },
            },
        ),
        Severity::Ignore => {}
    }
}
pub(crate) fn apply_suppression_findings(
    report: &mut GateReport,
    config: &HardgateConfig,
    role: FileRole,
    findings: Vec<SuppressionViolation>,
) {
    match severity(config, role) {
        Severity::Error => report.suppression_violations.extend(findings),
        Severity::Warning => apply_warning(
            report,
            WarningBatch {
                role,
                category: "suppression",
                findings,
                detail: |finding: &SuppressionViolation| {
                    (
                        finding.file.clone(),
                        format!("line {}: {}", finding.line_number, finding.message),
                    )
                },
            },
        ),
        Severity::Ignore => {}
    }
}
fn apply_warning<T, F>(report: &mut GateReport, batch: WarningBatch<T, F>)
where
    F: Fn(&T) -> (PathBuf, String),
{
    for finding in batch.findings {
        let (target, detail) = (batch.detail)(&finding);
        push_advisory(
            report,
            Advisory {
                role: batch.role,
                category: batch.category,
                target: &target,
                detail,
            },
        );
    }
}
pub(crate) fn apply_invariant_findings(
    report: &mut GateReport,
    config: &HardgateConfig,
    role: FileRole,
    findings: Vec<InvariantViolation>,
) {
    match severity(config, role) {
        Severity::Error => report.invariant_violations.extend(findings),
        Severity::Warning => apply_warning(
            report,
            WarningBatch {
                role,
                category: "invariant",
                findings,
                detail: |finding: &InvariantViolation| {
                    (
                        finding.file.clone(),
                        format!("line {}: {}", finding.line_number, finding.message),
                    )
                },
            },
        ),
        Severity::Ignore => {}
    }
}
pub(crate) fn apply_complexity_findings(
    report: &mut GateReport,
    config: &HardgateConfig,
    role: FileRole,
    findings: Vec<ComplexityViolation>,
) {
    match severity(config, role) {
        Severity::Error => report.complexity_violations.extend(findings),
        Severity::Warning => apply_warning(
            report,
            WarningBatch {
                role,
                category: "complexity",
                findings,
                detail: |finding: &ComplexityViolation| {
                    (
                        finding.file.clone(),
                        format!(
                            "{} `{}` at line {} (actual {:.0}, limit {:.0})",
                            finding.metric,
                            finding.function_name,
                            finding.line_number,
                            finding.actual,
                            finding.limit
                        ),
                    )
                },
            },
        ),
        Severity::Ignore => {}
    }
}
pub(crate) fn apply_clone_findings(
    report: &mut GateReport,
    config: &HardgateConfig,
    role: FileRole,
    findings: Vec<CloneViolation>,
) {
    match severity(config, role) {
        Severity::Error => report.clone_violations.extend(findings),
        Severity::Warning => apply_warning(
            report,
            WarningBatch {
                role,
                category: "clone",
                findings,
                detail: |finding: &CloneViolation| {
                    (
                        finding.file_a.clone(),
                        format!(
                            "{} ({} and {} lines, {} tokens)",
                            finding.message, finding.lines_a.0, finding.lines_b.0, finding.tokens
                        ),
                    )
                },
            },
        ),
        Severity::Ignore => {}
    }
}
pub(crate) fn apply_dead_code_findings(
    report: &mut GateReport,
    config: &HardgateConfig,
    role: FileRole,
    findings: Vec<DeadCodeViolation>,
) {
    match severity(config, role) {
        Severity::Error => report.dead_code_violations.extend(findings),
        Severity::Warning => apply_warning(
            report,
            WarningBatch {
                role,
                category: "dead code",
                findings,
                detail: |finding: &DeadCodeViolation| {
                    (
                        finding.file.clone(),
                        format!(
                            "{}{}: {}",
                            finding.violation_type,
                            finding
                                .line_number
                                .map(|line| format!(" at line {line}"))
                                .unwrap_or_default(),
                            finding.message
                        ),
                    )
                },
            },
        ),
        Severity::Ignore => {}
    }
}
pub(crate) struct CloneRun<'a> {
    pub read_results: &'a [(PathBuf, String)],
    pub changed_files: &'a [PathBuf],
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub diff: bool,
}
pub(crate) fn run_clone_analysis(input: CloneRun<'_>, report: &mut GateReport) -> Result<()> {
    if !input.config.clones.enabled {
        return Ok(());
    }
    let inputs = if input.diff {
        full_clone_inputs(input.config, input.root, report)?
    } else {
        clone_eligible_inputs(input.read_results, input.config)?
    };
    let mut groups: Vec<(FileRole, Vec<(PathBuf, String)>)> = FileRole::POLICY_ROLES
        .into_iter()
        .map(|role| (role, Vec::new()))
        .collect();
    for (file, content) in inputs {
        if let Some((_, group)) = groups.iter_mut().find(|(role, _)| *role == file.role) {
            group.push((file.path, content));
        }
    }
    for (role, files) in groups {
        run_clone_group(role, files, &input, report);
    }
    Ok(())
}
fn run_clone_group(
    role: FileRole,
    files: Vec<(PathBuf, String)>,
    input: &CloneRun<'_>,
    report: &mut GateReport,
) {
    let Some(clone_config) = clone_config_for_role(input.config, role) else {
        return;
    };
    let detector = CloneDetector::new(&clone_config);
    record_clone_exclusion_advisory(&detector, &files, input.root, report);
    if files.len() < 2 {
        return;
    }
    let result =
        detector.detect_clones_checked_with_changed_files(&files, input.root, input.changed_files);
    if let Err(ref error) = result {
        record_evidence_failure(
            report,
            true,
            EvidenceFailure {
                step: "clone-index",
                target: input.root,
                message: format!(
                    "role {role:?} clone index is incomplete: {error}. Raise clone thresholds or narrow this role's clone engine; do not add exclusions or suppressions."
                ),
            },
        );
        if let Some(failure) = report.orchestration_violations.last_mut() {
            failure.recommendation =
                "Raise clone thresholds or narrow this role's clone engine; do not add exclusions or suppressions.".to_string();
        }
        return;
    }
    let mut violations = result.expect("clone index result checked above");
    if input.diff {
        violations
            .retain(|violation| clone_touches_files(violation, input.changed_files, input.root));
    }
    apply_clone_findings(report, input.config, role, violations);
}
fn clone_eligible_inputs(
    read_results: &[(PathBuf, String)],
    config: &HardgateConfig,
) -> Result<Vec<(ClassifiedFile, String)>> {
    read_results
        .iter()
        .map(|(path, content)| Ok((classify_file(path, config)?, content.clone())))
        .collect::<Result<Vec<_>>>()
        .map(|files| {
            files
                .into_iter()
                .filter(|(file, _)| file.role.receives_clone_analysis())
                .collect()
        })
}
fn full_clone_inputs(
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) -> Result<Vec<(ClassifiedFile, String)>> {
    let discovery =
        crate::discovery::discover_files_with_exclusions(crate::discovery::DiscoverOptions {
            root,
            diff_only: false,
            exclusions: &config.budgets.files.exclusions.paths,
        })?;
    let files = classify_files(&discovery.files, config)?
        .into_iter()
        .filter(|file| file.role.receives_clone_analysis())
        .collect::<Vec<_>>();
    Ok(read_clone_files(&files, config, report))
}
fn read_clone_files(
    files: &[ClassifiedFile],
    config: &HardgateConfig,
    report: &mut GateReport,
) -> Vec<(ClassifiedFile, String)> {
    let mut read = Vec::new();
    for file in files {
        match fs::read_to_string(&file.path) {
            Ok(content) => read.push((file.clone(), content)),
            Err(error) => record_role_evidence_failure(
                report,
                RoleEvidence {
                    config,
                    role: file.role,
                    step: "read-clone-index",
                    target: &file.path,
                    message: format!("Unable to read file required by full clone index: {error}"),
                },
            ),
        }
    }
    read
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
fn clone_touches_files(violation: &CloneViolation, files: &[PathBuf], root: &Path) -> bool {
    let file_a = crate::engines::clones::repository_relative_path(&violation.file_a, root);
    let file_b = crate::engines::clones::repository_relative_path(&violation.file_b, root);
    files.iter().any(|path| {
        let changed = crate::engines::clones::repository_relative_path(path, root);
        changed == file_a || changed == file_b
    })
}
