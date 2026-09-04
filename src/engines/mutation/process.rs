use super::runner::MutantOutcome;
use crate::engines::process::{ProcessOutcome, append_output, run_command};
use std::process::ExitStatus;
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct CommandExecution {
    pub outcome: MutantOutcome,
    pub diagnostic: String,
}

pub(crate) fn execute_with_timeout(
    command: &str,
    root: &std::path::Path,
    timeout_secs: u64,
) -> CommandExecution {
    let tokens = crate::engines::orchestration::shell_words_split(command);
    if tokens.is_empty() {
        return command_error("Empty command string; nothing was executed.".to_string());
    }
    let outcome = run_command(
        &tokens,
        root,
        Duration::from_secs(timeout_secs.max(1)),
        "mutation",
    );
    finish_outcome(outcome)
}

fn command_error(diagnostic: String) -> CommandExecution {
    CommandExecution {
        outcome: MutantOutcome::RunnerError,
        diagnostic,
    }
}

fn finish_outcome(outcome: ProcessOutcome) -> CommandExecution {
    match outcome {
        ProcessOutcome::Completed { status, output } => CommandExecution {
            outcome: outcome_from_status(&status, &output),
            diagnostic: output,
        },
        ProcessOutcome::TimedOut { output } => CommandExecution {
            outcome: MutantOutcome::Timeout,
            diagnostic: output,
        },
        ProcessOutcome::Failed { message, output } => CommandExecution {
            outcome: MutantOutcome::RunnerError,
            diagnostic: append_output(output, message),
        },
    }
}

fn outcome_from_status(status: &ExitStatus, diagnostic: &str) -> MutantOutcome {
    if status.success() {
        MutantOutcome::Survived
    } else if looks_like_compile_error(diagnostic) {
        MutantOutcome::CompileError
    } else {
        MutantOutcome::Killed
    }
}

fn looks_like_compile_error(diagnostic: &str) -> bool {
    let lower = diagnostic.to_ascii_lowercase();
    [
        "could not compile",
        "compilation failed",
        "compile error",
        "compile_error",
        "syntaxerror",
        "syntax error",
        "failed to parse",
        "error[e",
        "error ts",
        "typecheck failed",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}
