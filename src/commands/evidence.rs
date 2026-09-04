use crate::diagnostics::GateReport;
use crate::engines::OrchestrationViolation;
use std::path::Path;

/// Record missing or invalid evidence as a blocking finding in strict mode,
/// or as a visible advisory in adoption-oriented modes.
pub(crate) fn record_evidence_failure(
    report: &mut GateReport,
    blocking: bool,
    step: &str,
    target: &Path,
    message: String,
) {
    if !blocking {
        report
            .advisories
            .push(format!("{} for `{}`: {}", step, target.display(), message));
        return;
    }
    report.orchestration_violations.push(OrchestrationViolation {
        step: step.to_string(),
        command: target.display().to_string(),
        exit_code: None,
        output: message,
        recommendation:
            "Restore the required evidence or classify the file explicitly before accepting the gate."
                .to_string(),
    });
}
