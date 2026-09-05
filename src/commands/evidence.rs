use crate::diagnostics::GateReport;
use crate::engines::OrchestrationViolation;
use std::path::Path;

pub(crate) struct EvidenceFailure<'a> {
    pub step: &'a str,
    pub target: &'a Path,
    pub message: String,
}

/// Record missing or invalid evidence as a blocking finding in strict mode,
/// or as a visible advisory in adoption-oriented modes.
pub(crate) fn record_evidence_failure(
    report: &mut GateReport,
    blocking: bool,
    failure: EvidenceFailure<'_>,
) {
    let EvidenceFailure {
        step,
        target,
        message,
    } = failure;
    report.observe_evidence_failure(step, &message);
    if !blocking {
        report
            .advisories
            .push(format!("{} for `{}`: {}", step, target.display(), message));
        return;
    }
    report
        .orchestration_violations
        .push(OrchestrationViolation {
            step: step.to_string(),
            command: target.display().to_string(),
            exit_code: None,
            output: message,
            recommendation: remediation(step).to_string(),
        });
}

fn remediation(step: &str) -> &'static str {
    match step {
        "coverage-report" | "coverage-diff" | "coverage-source-classification" => {
            "Generate current line/function/branch LCOV for the selected source and configure coverage.report (or --coverage-report); preserve missing-source failures."
        }
        "mutation-report" => {
            "Run the configured mutation tool against current source with a successful baseline and non-empty sample; set mutation.reports (or verify --mutation-report)."
        }
        "legacy-ratchet" => {
            "Set legacy.reference_branch to a resolvable, trusted Git reference and retain its source history."
        }
        "clone-index" | "read-clone-index" => {
            "Restore readable clone-index inputs and report capacity failures; do not remove source or relax policy to pass."
        }
        _ => {
            "Restore readable, valid source and supported analysis evidence before accepting the gate."
        }
    }
}
