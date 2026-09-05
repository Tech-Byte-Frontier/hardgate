use crate::commands::evidence::{EvidenceFailure, record_evidence_failure};
use crate::commands::role_policy::{RoleEvidence, record_role_evidence_failure};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{ClassifiedFile, FileRole};
use std::path::Path;

pub(super) fn record_classification_gaps(
    files: &[&ClassifiedFile],
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
    } else if file.role == FileRole::Source && !file.ast_supported {
        if crate::discovery::classification::is_inventory_file(&file.path) {
            report.advisories.push(format!(
                "role Source: `{}` is a recognized inventory source without AST parser; validated for file budgets, suppressions, and invariants.",
                rel.display()
            ));
        } else {
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
    } else if file.role == FileRole::Migration && !file.ast_supported {
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
