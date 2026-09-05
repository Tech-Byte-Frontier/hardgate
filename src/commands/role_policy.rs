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
use crate::discovery::classification::PreparedClassifier;
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

pub(crate) fn classify_file(
    path: &Path,
    config: &HardgateConfig,
    root: &Path,
) -> Result<ClassifiedFile> {
    let classifier = PreparedClassifier::new(&config.classification)?;
    Ok(classify_prepared(path, &classifier, root))
}

pub(crate) fn classify_files(
    paths: &[PathBuf],
    config: &HardgateConfig,
    root: &Path,
) -> Result<Vec<ClassifiedFile>> {
    let classifier = PreparedClassifier::new(&config.classification)?;
    Ok(paths
        .iter()
        .map(|path| classify_prepared(path, &classifier, root))
        .collect())
}

fn classify_prepared(path: &Path, classifier: &PreparedClassifier, root: &Path) -> ClassifiedFile {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut file = classifier.classify(relative);
    file.path = path.to_path_buf();
    file
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
    report.observe_evidence_failure(failure.step, &failure.message);
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

#[cfg(test)]
mod tests {
    use super::{clone_config_for_role, effective_file_budgets, effective_function_budgets};
    use crate::commands::run_static_gate_snapshot;
    use crate::config::{FunctionBudgets, HardgateConfig};
    use crate::discovery::FileRole;
    use std::path::PathBuf;

    fn snapshot(config: &HardgateConfig) -> crate::diagnostics::GateReport {
        let files = vec![
            (
                PathBuf::from("src/source.rs"),
                "fn source() {\n    let value = 1;\n    value;\n}\n".to_string(),
            ),
            (
                PathBuf::from("tests/sibling.rs"),
                "fn sibling() {\n    let value = 1;\n    value;\n}\n".to_string(),
            ),
        ];
        run_static_gate_snapshot(config, &files)
            .expect("role-policy snapshot should analyze")
            .0
    }

    #[test]
    fn source_file_limits_override_global_without_affecting_sibling_role() {
        let mut config = HardgateConfig::default();
        config.clones.enabled = false;
        config.budgets.files.max_bytes = Some(1_000);
        config.budgets.files.max_lines.insert("rs".to_string(), 100);
        config.roles.source.max_bytes = Some(1);
        config.roles.source.max_lines = Some(1);

        let report = snapshot(&config);
        assert_eq!(report.budget_violations.len(), 2);
        assert!(
            report
                .budget_violations
                .iter()
                .all(|finding| finding.file == PathBuf::from("src/source.rs"))
        );

        let source = effective_file_budgets(&config, FileRole::Source);
        assert_eq!(source.max_bytes, Some(1));
        assert_eq!(source.max_lines.get("rs"), Some(&1));
        assert_eq!(source.max_lines.get("default"), Some(&1));

        let sibling = effective_file_budgets(&config, FileRole::Test);
        assert_eq!(sibling.max_bytes, Some(1_000));
        assert_eq!(sibling.max_lines.get("rs"), Some(&100));
        assert_eq!(config.budgets.files.max_bytes, Some(1_000));
        assert_eq!(config.budgets.files.max_lines.get("rs"), Some(&100));

        let config_role = effective_file_budgets(&config, FileRole::Config);
        assert_eq!(config_role.max_bytes, Some(1_000));
        assert_eq!(config_role.max_lines.get("rs"), Some(&100));
    }

    #[test]
    fn function_limits_override_all_metrics_and_no_policy_roles_inherit_global() {
        let mut config = HardgateConfig::default();
        config.budgets.functions = FunctionBudgets {
            max_cyclomatic: Some(90),
            max_cognitive: Some(91),
            max_halstead_difficulty: Some(92.0),
            max_abc: Some(93.0),
            max_parameters: Some(94),
            max_lines: Some(95),
            max_statements: Some(96),
            max_nesting_depth: Some(97),
        };
        config.roles.source.max_cyclomatic = Some(1);
        config.roles.source.max_cognitive = Some(2);
        config.roles.source.max_halstead_difficulty = Some(3.0);
        config.roles.source.max_abc = Some(4.0);
        config.roles.source.max_parameters = Some(5);
        config.roles.source.max_function_lines = Some(6);
        config.roles.source.max_statements = Some(7);
        config.roles.source.max_nesting_depth = Some(8);

        let source = effective_function_budgets(&config, FileRole::Source);
        assert_eq!(source.max_cyclomatic, Some(1));
        assert_eq!(source.max_cognitive, Some(2));
        assert_eq!(source.max_halstead_difficulty, Some(3.0));
        assert_eq!(source.max_abc, Some(4.0));
        assert_eq!(source.max_parameters, Some(5));
        assert_eq!(source.max_lines, Some(6));
        assert_eq!(source.max_statements, Some(7));
        assert_eq!(source.max_nesting_depth, Some(8));

        let sibling = effective_function_budgets(&config, FileRole::Test);
        assert_eq!(sibling.max_cyclomatic, Some(90));
        assert_eq!(sibling.max_cognitive, Some(91));
        assert_eq!(sibling.max_halstead_difficulty, Some(92.0));
        assert_eq!(sibling.max_abc, Some(93.0));
        assert_eq!(sibling.max_parameters, Some(94));
        assert_eq!(sibling.max_lines, Some(95));
        assert_eq!(sibling.max_statements, Some(96));
        assert_eq!(sibling.max_nesting_depth, Some(97));

        let config_role = effective_function_budgets(&config, FileRole::Config);
        assert_eq!(config_role.max_parameters, Some(94));
        assert_eq!(config_role.max_nesting_depth, Some(97));
    }

    #[test]
    fn no_first_class_clone_role_uses_global_clone_policy() {
        let mut config = HardgateConfig::default();
        config.clones.enabled = true;
        config.clones.min_lines = 17;
        config.clones.min_tokens = 170;

        let config_role = clone_config_for_role(&config, FileRole::Config)
            .expect("enabled global clone policy should reach config role");
        assert_eq!(config_role.min_lines, 17);
        assert_eq!(config_role.min_tokens, 170);
    }

    #[test]
    fn zero_role_thresholds_remain_invalid_configuration() {
        let mut config = HardgateConfig::default();
        config.roles.source.max_function_lines = Some(0);
        assert!(config.validate().is_err());
    }
}
