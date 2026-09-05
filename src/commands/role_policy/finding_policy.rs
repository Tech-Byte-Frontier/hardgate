use super::{FileRole, severity};
use crate::config::{HardgateConfig, Severity};
use crate::engines::{CloneViolation, ComplexityViolation};

pub(super) fn file_size_severity(config: &HardgateConfig, role: FileRole) -> Severity {
    config
        .roles
        .for_role(role)
        .and_then(|policy| policy.file_size_severity)
        .unwrap_or_else(|| severity(config, role))
}

pub(super) fn complexity_severity(
    config: &HardgateConfig,
    role: FileRole,
    finding: &ComplexityViolation,
) -> Severity {
    if matches!(
        finding.metric.as_str(),
        "Function Lines" | "Statement Count"
    ) {
        return config
            .roles
            .for_role(role)
            .and_then(|policy| policy.function_size_severity)
            .unwrap_or_else(|| severity(config, role));
    }
    severity(config, role)
}

pub(super) fn clone_severity(
    config: &HardgateConfig,
    role: FileRole,
    finding: &CloneViolation,
) -> Severity {
    let Some(policy) = config.roles.for_role(role) else {
        return severity(config, role);
    };
    let configured = policy
        .clone_severity
        .unwrap_or_else(|| severity(config, role));
    if configured == Severity::Error
        && (policy
            .clone_block_min_lines
            .is_some_and(|minimum| finding.lines < minimum)
            || policy
                .clone_block_min_tokens
                .is_some_and(|minimum| finding.tokens < minimum))
    {
        return Severity::Warning;
    }
    configured
}

pub(super) fn partition<T>(
    findings: Vec<T>,
    severity_for: impl Fn(&T) -> Severity,
) -> (Vec<T>, Vec<T>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for finding in findings {
        match severity_for(&finding) {
            Severity::Error => errors.push(finding),
            Severity::Warning => warnings.push(finding),
            Severity::Ignore => {}
        }
    }
    (errors, warnings)
}

#[cfg(test)]
#[path = "finding_policy_tests.rs"]
mod tests;
