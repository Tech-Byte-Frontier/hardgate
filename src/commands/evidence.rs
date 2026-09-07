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
            "Run `hardgate evidence vitest` for JS/TS or `hardgate evidence cargo-llvm-cov --toolchain <installed-nightly>` for Rust; set coverage.report to .hardgate/evidence/coverage.lcov (or use --coverage-report). Coverage written by check commands is disposable and cannot issue a producer receipt."
        }
        "mutation-report" => {
            "Run `hardgate evidence cargo-mutants` or `hardgate evidence stryker` against current source; set mutation.reports (or check --mutation-report)."
        }
        "parse-source" => {
            "Run the project compiler or type check first. If it accepts this file, report unsupported Hardgate parser syntax with the line/column; an equivalent imported type alias may help for TypeScript. Hardgate still requires complete AST evidence."
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
