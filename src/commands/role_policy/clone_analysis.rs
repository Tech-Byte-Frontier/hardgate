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
    pub ownership: &'a crate::discovery::rust_ownership::RustOwnership,
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

fn clone_group(
    input: &CloneRun<'_>,
    role: FileRole,
    selected: &HashSet<FileId>,
    report: &mut GateReport,
) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    for source in &input.snapshot.files {
        if (!input.diff && !selected.contains(&source.id))
            || !input.ownership.has_role(&source.classified, role)
        {
            continue;
        }
        match &source.content {
            Ok(text) => files.extend(
                input
                    .ownership
                    .views(&source.classified, text)
                    .into_iter()
                    .filter(|view| view.file.role == role)
                    .map(|view| (view.file.path, view.text)),
            ),
            Err(error) if input.ownership.file_role(&source.classified) == role => {
                record_role_evidence_failure(
                    report,
                    RoleEvidence {
                        config: input.config,
                        role,
                        step: "read-clone-index",
                        target: &source.classified.path,
                        message: format!(
                            "Unable to read file required by full clone index: {error}"
                        ),
                    },
                )
            }
            Err(_) => {}
        }
    }
    files
}

struct CloneGroup {
    role: FileRole,
    files: Vec<(PathBuf, String)>,
    detector: CloneDetector,
}

fn run_clone_group(group: CloneGroup, input: &CloneRun<'_>, report: &mut GateReport) {
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
    if files.is_empty() {
        return;
    }
    report.observe_engine(
        crate::diagnostics::execution::EngineId::Clones,
        crate::diagnostics::execution::EngineState::Completed,
    );
    match detector.detect_clones_checked_with_changed_files(&files, input.root, input.changed_files)
    {
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
