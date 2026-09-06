use super::finding_policy::{clone_severity, complexity_severity, file_size_severity, partition};
use super::{Advisory, push_advisory, severity};
use crate::config::{HardgateConfig, Severity};
use crate::diagnostics::GateReport;
use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, InvariantViolation, SuppressionViolation,
};
use std::path::PathBuf;

struct WarningBatch<T, F> {
    role: super::FileRole,
    category: &'static str,
    findings: Vec<T>,
    detail: F,
}

pub(crate) fn apply_budget_findings(
    report: &mut GateReport,
    config: &HardgateConfig,
    role: super::FileRole,
    findings: Vec<BudgetViolation>,
) {
    match file_size_severity(config, role) {
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
    role: super::FileRole,
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
    role: super::FileRole,
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
    role: super::FileRole,
    findings: Vec<ComplexityViolation>,
) {
    let (errors, warnings) = partition(findings, |finding| {
        complexity_severity(config, role, finding)
    });
    report.complexity_violations.extend(errors);
    apply_warning(
        report,
        WarningBatch {
            role,
            category: "complexity",
            findings: warnings,
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
    );
}

pub(crate) fn apply_clone_findings(
    report: &mut GateReport,
    config: &HardgateConfig,
    role: super::FileRole,
    findings: Vec<CloneViolation>,
) {
    let (errors, warnings) = partition(findings, |finding| clone_severity(config, role, finding));
    report.clone_violations.extend(errors);
    apply_warning(
        report,
        WarningBatch {
            role,
            category: "clone",
            findings: warnings,
            detail: |finding: &CloneViolation| {
                (
                    finding.file_a.clone(),
                    format!(
                        "Detected duplication at lines {}-{} with `{}:{}-{}` ({} lines, {} tokens, fingerprint {}); advisory under the configured clone policy.",
                        finding.lines_a.0,
                        finding.lines_a.1,
                        finding.file_b.display(),
                        finding.lines_b.0,
                        finding.lines_b.1,
                        finding.lines,
                        finding.tokens,
                        finding.fingerprint
                    ),
                )
            },
        },
    );
}
