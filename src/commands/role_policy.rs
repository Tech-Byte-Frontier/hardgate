mod clone_analysis;
mod findings;

pub(crate) use clone_analysis::{CloneRun, run_clone_analysis};
pub(crate) use findings::{
    apply_budget_findings, apply_complexity_findings, apply_dead_code_findings,
    apply_invariant_findings, apply_suppression_findings,
};

use super::evidence::{EvidenceFailure, record_evidence_failure};
use crate::config::{CloneConfig, FileBudgets, FunctionBudgets, HardgateConfig, Severity};
use crate::diagnostics::GateReport;
use crate::discovery::{ClassifiedFile, FileRole};
use anyhow::Result;
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
    let policy = config.roles.for_role(role);
    let enabled = policy
        .and_then(|policy| policy.clone_enabled)
        .unwrap_or(config.clones.enabled);
    if !enabled {
        return None;
    }
    let mut clone = config.clones.clone();
    let Some(policy) = policy else {
        return Some(clone);
    };
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
