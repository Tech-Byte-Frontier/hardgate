use super::super::evidence::{EvidenceFailure, record_evidence_failure};
use super::findings::apply_clone_findings;
use super::{
    RoleEvidence, classify_file, classify_files, clone_config_for_role,
    record_role_evidence_failure,
};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{ClassifiedFile, FileRole};
use crate::engines::{CloneDetector, CloneViolation};
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct CloneRun<'a> {
    pub read_results: &'a [(PathBuf, String)],
    pub changed_files: &'a [PathBuf],
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub diff: bool,
}

pub(crate) fn run_clone_analysis(input: CloneRun<'_>, report: &mut GateReport) -> Result<()> {
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
                "Raise clone thresholds or narrow this role's clone engine; do not add exclusions or suppressions."
                    .to_string();
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
                .filter(|(file, _)| clone_input_is_eligible(file, config))
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
        .filter(|file| clone_input_is_eligible(file, config))
        .collect::<Vec<_>>();
    Ok(read_clone_files(&files, config, report))
}

fn clone_input_is_eligible(file: &ClassifiedFile, config: &HardgateConfig) -> bool {
    file.role.receives_clone_analysis()
        || config
            .roles
            .for_role(file.role)
            .and_then(|policy| policy.clone_enabled)
            == Some(true)
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
    report: &mut crate::diagnostics::GateReport,
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
