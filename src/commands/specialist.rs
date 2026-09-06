use crate::diagnostics::GateReport;
use crate::diagnostics::execution::{EngineState, evidence_engine};
use crate::engines::cargo_diagnostics;
use crate::engines::orchestration::{
    OrchestrationResult, OrchestrationViolation, shell_words_split,
};
use std::path::Path;

enum ClippyState {
    NotApplicable,
    Finished(EngineState),
    Incomplete,
}

pub(super) fn record(
    report: &mut GateReport,
    result: Result<OrchestrationResult, OrchestrationViolation>,
    root: &Path,
) {
    match clippy_state(report, &result, root) {
        ClippyState::Finished(state) => {
            report.observe_engine(evidence_engine("lint"), state);
            return;
        }
        ClippyState::Incomplete => {
            if let Ok(value) = result {
                report.orchestration_violations.push(OrchestrationViolation {
                    step: value.step,
                    command: value.command,
                    exit_code: None,
                    output: "Clippy diagnostic stream is incomplete; finding counts and completion cannot be verified.".into(),
                    recommendation: "Resolve the incomplete Cargo JSON output before accepting this check.".into(),
                });
                return;
            }
        }
        ClippyState::NotApplicable => {}
    }
    match result {
        Ok(value) => report.observe_engine(evidence_engine(&value.step), EngineState::Completed),
        Err(failure) => report.orchestration_violations.push(failure),
    }
}

fn clippy_state(
    report: &mut GateReport,
    result: &Result<OrchestrationResult, OrchestrationViolation>,
    root: &Path,
) -> ClippyState {
    let (step, command, output, exit) = match result {
        Ok(value) => (&value.step, &value.command, &value.output, Some(0)),
        Err(value) => (&value.step, &value.command, &value.output, value.exit_code),
    };
    if step != "lint" || !cargo_diagnostics::is_clippy(&shell_words_split(command)) {
        return ClippyState::NotApplicable;
    }
    let parsed = cargo_diagnostics::parse(output, root);
    let state = completed_state(&parsed, exit);
    report.tool_diagnostics.extend(parsed.findings);
    state
}

fn completed_state(parsed: &cargo_diagnostics::CargoDiagnostics, exit: Option<i32>) -> ClippyState {
    let has_error = parsed.findings.iter().any(|finding| finding.blocking);
    let finished = parsed.complete && exit.is_some_and(|code| parsed.success == Some(code == 0));
    if finished && (exit == Some(0) || has_error) {
        ClippyState::Finished(if has_error {
            EngineState::Failed
        } else {
            EngineState::Completed
        })
    } else {
        ClippyState::Incomplete
    }
}
