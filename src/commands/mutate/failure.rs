use super::MutationRenderOptions;
use crate::commands::outcome::{write_atomic_file, write_stdout};
use crate::diagnostics::execution::ExecutionPlan;
use crate::engines::mutation::runner::MutationRunnerError;
use crate::engines::mutation::{BaselineExecutionResult, BaselineOutcome};
use crate::engines::{MutantExecutionResult, MutantOutcome};
use colored::*;
use serde::Serialize;
use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub struct MutationFailure {
    pub stage: &'static str,
    pub kind: &'static str,
    pub message: String,
}

impl MutationFailure {
    pub(crate) fn new(stage: &'static str, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            stage,
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn from_runner_error(error: MutationRunnerError) -> Self {
        match error {
            MutationRunnerError::Resolution(message) => {
                Self::new("resolution", "resolution-error", message)
            }
            MutationRunnerError::Integrity(message) => {
                Self::new("execution", "execution-error", message)
            }
        }
    }
}

impl fmt::Display for MutationFailure {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(&self.message)
    }
}

impl std::error::Error for MutationFailure {}

#[derive(Serialize)]
pub(crate) struct MutationNoop<'a> {
    pub passed: bool,
    pub status: &'static str,
    pub stage: &'static str,
    pub kind: &'static str,
    pub message: &'a str,
}

const DISABLED_MUTATION_MESSAGE: &str =
    "mutation testing is disabled by `[mutation].enabled = false`.";
const NO_CHANGED_TARGETS_MESSAGE: &str =
    "no git-modified files found for mutation testing; no changed production source targets.";
const DISABLED_MUTATION_NOTICE: MutationNoopNotice = MutationNoopNotice {
    stage: "policy",
    kind: "disabled",
    message: DISABLED_MUTATION_MESSAGE,
    note: DISABLED_MUTATION_MESSAGE,
};
const NO_CHANGED_TARGETS_NOTICE: MutationNoopNotice = MutationNoopNotice {
    stage: "selection",
    kind: "no-changed-targets",
    message: NO_CHANGED_TARGETS_MESSAGE,
    note: "no git-modified files found for mutation testing; no changed production source targets (no-op).",
};

struct MutationNoopNotice {
    stage: &'static str,
    kind: &'static str,
    message: &'static str,
    note: &'static str,
}

pub(crate) fn render_mutation_noop(
    noop: MutationNoop<'_>,
    opts: MutationRenderOptions<'_>,
    execution: Option<&ExecutionPlan>,
) -> anyhow::Result<()> {
    let output = if opts.format == Some("json") {
        format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1, "command": "mutate", "exit_code": 0, "execution": execution,
                "passed": noop.passed, "status": noop.status, "stage": noop.stage, "kind": noop.kind, "message": noop.message,
            }))?
        )
    } else {
        format!("{} {}\n", "note:".green().bold(), noop.message)
    };
    if let Some(path) = opts.output_file {
        write_atomic_file(path, &output)?;
    }
    write_stdout(&output)?;
    Ok(())
}

pub(crate) fn finish_disabled_mutation(
    opts: MutationRenderOptions<'_>,
    execution: &ExecutionPlan,
) -> anyhow::Result<()> {
    render_noop_or_note(opts, DISABLED_MUTATION_NOTICE, execution)
}

pub(crate) fn handle_no_targets(
    diff: bool,
    opts: MutationRenderOptions<'_>,
    execution: &ExecutionPlan,
) -> anyhow::Result<()> {
    if !diff {
        return Err(MutationFailure::new(
            "setup",
            "setup-error",
            "no source files found for mutation testing: no production source files are eligible; full/native runs require at least one production target",
        )
        .into());
    }
    render_noop_or_note(opts, NO_CHANGED_TARGETS_NOTICE, execution)
}

fn render_noop_or_note(
    opts: MutationRenderOptions<'_>,
    notice: MutationNoopNotice,
    execution: &ExecutionPlan,
) -> anyhow::Result<()> {
    let message = if opts.format == Some("json") {
        notice.message
    } else {
        notice.note
    };
    render_mutation_noop(
        MutationNoop {
            passed: true,
            status: "noop",
            stage: notice.stage,
            kind: notice.kind,
            message,
        },
        opts,
        Some(execution),
    )
}

pub(crate) fn baseline_failure(result: &BaselineExecutionResult, file: &Path) -> anyhow::Error {
    let diagnostic = if result.diagnostic.trim().is_empty() {
        "no diagnostic output".to_string()
    } else {
        result.diagnostic.clone()
    };
    let kind = match result.outcome {
        BaselineOutcome::Failed => "test-failure",
        BaselineOutcome::Timeout => "timeout",
        BaselineOutcome::RunnerError => "runner-error",
        BaselineOutcome::Passed => "test-failure",
    };
    MutationFailure::new(
        "baseline",
        kind,
        format!(
            "unmutated baseline {:?} for `{}` using `{}`:\n{}",
            result.outcome,
            file.display(),
            result.command,
            diagnostic
        ),
    )
    .into()
}

pub(crate) fn runtime_failure(result: &MutantExecutionResult) -> Option<anyhow::Error> {
    let kind = match result.outcome {
        MutantOutcome::RunnerError => "execution-error",
        MutantOutcome::Timeout => "timeout",
        MutantOutcome::Killed
        | MutantOutcome::Survived
        | MutantOutcome::CompileError
        | MutantOutcome::Equivalent
        | MutantOutcome::Unviable => return None,
    };
    Some(
        MutationFailure::new(
            "execution",
            kind,
            format!(
                "mutant {} {:?} for `{}`: {}",
                result.mutant.id,
                result.outcome,
                result.mutant.file.display(),
                if result.diagnostic.trim().is_empty() {
                    "no diagnostic output"
                } else {
                    result.diagnostic.as_str()
                }
            ),
        )
        .into(),
    )
}
