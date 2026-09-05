use super::super::evidence::{EvidenceFailure, record_evidence_failure};
use super::findings::apply_clone_findings;
use super::{RoleEvidence, clone_config_for_role, record_role_evidence_failure};
use crate::commands::source_snapshot::{FileId, SourceSnapshot};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::FileRole;
use crate::engines::{CloneDetector, CloneViolation};
use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(crate) struct CloneRun<'a> {
    pub snapshot: &'a SourceSnapshot,
    pub selected_ids: &'a [FileId],
    pub changed_files: &'a [PathBuf],
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub diff: bool,
}

pub(crate) fn run_clone_analysis(input: CloneRun<'_>, report: &mut GateReport) -> Result<()> {
    let selected: HashSet<FileId> = input.selected_ids.iter().copied().collect();
    for role in FileRole::POLICY_ROLES {
        let Some(config) = clone_config_for_role(input.config, role) else {
            continue;
        };
        if !role.receives_clone_analysis()
            && input
                .config
                .roles
                .for_role(role)
                .and_then(|policy| policy.clone_enabled)
                != Some(true)
        {
            continue;
        }
        let files = clone_group(&input, role, &selected, report);
        run_clone_group(
            CloneGroup {
                role,
                files,
                detector: CloneDetector::new(&config),
            },
            &input,
            report,
        );
    }
    Ok(())
}

fn clone_group<'a>(
    input: &'a CloneRun<'_>,
    role: FileRole,
    selected: &HashSet<FileId>,
    report: &mut GateReport,
) -> Vec<(PathBuf, &'a str)> {
    let mut files = Vec::new();
    for source in &input.snapshot.files {
        if source.classified.role != role || (!input.diff && !selected.contains(&source.id)) {
            continue;
        }
        match &source.content {
            Ok(text) => files.push((source.classified.path.clone(), text.as_ref())),
            Err(error) => record_role_evidence_failure(
                report,
                RoleEvidence {
                    config: input.config,
                    role,
                    step: "read-clone-index",
                    target: &source.classified.path,
                    message: format!("Unable to read file required by full clone index: {error}"),
                },
            ),
        }
    }
    files
}

struct CloneGroup<'a> {
    role: FileRole,
    files: Vec<(PathBuf, &'a str)>,
    detector: CloneDetector,
}

fn run_clone_group(group: CloneGroup<'_>, input: &CloneRun<'_>, report: &mut GateReport) {
    let CloneGroup {
        role,
        files,
        detector,
    } = group;
    let count = files
        .iter()
        .filter(|(path, _)| detector.excludes_path(path, input.root))
        .count();
    if count > 0 {
        let noun = if count == 1 { "file" } else { "files" };
        report.advisories.push(format!(
            "{count} {noun} excluded from clone detection via hardgate.toml."
        ));
    }
    if files.len() < 2 {
        return;
    }
    match detector.detect_clones_borrowed(&files, input.root, input.changed_files) {
        Ok(mut findings) => {
            if input.diff {
                findings.retain(|finding| {
                    clone_touches_files(finding, input.changed_files, input.root)
                });
            }
            apply_clone_findings(report, input.config, role, findings);
        }
        Err(error) => {
            record_evidence_failure(
                report,
                true,
                EvidenceFailure {
                    step: "clone-index",
                    target: input.root,
                    message: format!(
                        "role {role:?} clone index is incomplete: {error}. Retain the failing status and report this input pattern; do not omit source or weaken policy to obtain a pass."
                    ),
                },
            );
            if let Some(failure) = report.orchestration_violations.last_mut() {
                failure.recommendation = "Retain the failing status and report the input pattern and capacity error; do not weaken policy or omit source to obtain a pass.".to_string();
            }
        }
    }
}

fn clone_touches_files(violation: &CloneViolation, files: &[PathBuf], root: &Path) -> bool {
    let file_a = crate::engines::clones::repository_relative_path(&violation.file_a, root);
    let file_b = crate::engines::clones::repository_relative_path(&violation.file_b, root);
    files.iter().any(|path| {
        let changed = crate::engines::clones::repository_relative_path(path, root);
        changed == file_a || changed == file_b
    })
}
