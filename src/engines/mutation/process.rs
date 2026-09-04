use super::runner::MutantOutcome;
use crate::engines::process::{
    CommandRoots, ProcessOutcome, append_output, run_command_with_roots, timeout_scope,
};
use std::process::ExitStatus;
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct CommandExecution {
    pub outcome: MutantOutcome,
    pub diagnostic: String,
    pub status: Option<ExitStatus>,
}

pub(crate) fn execute_with_timeout(
    command: &str,
    roots: CommandRoots<'_>,
    timeout_secs: u64,
) -> CommandExecution {
    let tokens = crate::engines::orchestration::shell_words_split(command);
    if tokens.is_empty() {
        return command_error("Empty command string; nothing was executed.".to_string());
    }
    let outcome = run_command_with_roots(
        &tokens,
        roots,
        Duration::from_secs(timeout_secs.max(1)),
        "mutation",
    );
    finish_outcome(outcome)
}

fn command_error(diagnostic: String) -> CommandExecution {
    CommandExecution {
        outcome: MutantOutcome::RunnerError,
        diagnostic,
        status: None,
    }
}

fn finish_outcome(outcome: ProcessOutcome) -> CommandExecution {
    match outcome {
        ProcessOutcome::Completed { status, output } => CommandExecution {
            outcome: outcome_from_status(&status, &output),
            diagnostic: output,
            status: Some(status),
        },
        ProcessOutcome::TimedOut { output } => CommandExecution {
            outcome: MutantOutcome::Timeout,
            diagnostic: append_output(
                output,
                format!(
                    "Command timed out; {scope} terminated and absence was verified.",
                    scope = timeout_scope()
                ),
            ),
            status: None,
        },
        ProcessOutcome::Failed { message, output } => CommandExecution {
            outcome: MutantOutcome::RunnerError,
            diagnostic: append_output(output, message),
            status: None,
        },
    }
}

fn outcome_from_status(status: &ExitStatus, diagnostic: &str) -> MutantOutcome {
    if status.success() {
        MutantOutcome::Survived
    } else if status_is_runner_error(status) {
        MutantOutcome::RunnerError
    } else if looks_like_compile_error(diagnostic) {
        MutantOutcome::CompileError
    } else if looks_like_test_failure(diagnostic) {
        MutantOutcome::Killed
    } else {
        MutantOutcome::RunnerError
    }
}

fn status_is_runner_error(status: &ExitStatus) -> bool {
    match status.code() {
        None => true,
        Some(code) => matches!(code, 126 | 127) || code >= 128,
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

fn looks_like_test_failure(diagnostic: &str) -> bool {
    diagnostic.lines().any(test_failure_line)
}

fn test_failure_line(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    framework_failure_line(trimmed, &lower)
        || rust_failure_line(&lower)
        || assertion_failure_line(&lower)
        || playwright_failure_line(&lower)
}

fn framework_failure_line(trimmed: &str, lower: &str) -> bool {
    trimmed == "FAIL"
        || trimmed.starts_with("FAIL ")
        || trimmed.starts_with("FAIL\t")
        || lower.starts_with("test suites:") && lower.contains(" failed")
        || lower.starts_with("test files:") && lower.contains(" failed")
        || lower.starts_with("test files ") && lower.contains(" failed")
}

fn rust_failure_line(lower: &str) -> bool {
    lower == "fail"
        || lower.starts_with("failures:")
        || lower.starts_with("test result: failed")
        || lower.contains("... failed")
        || lower.starts_with("panicked")
        || lower.contains(" panicked")
        || lower.starts_with("panic:")
        || lower.starts_with("panic!")
}

fn assertion_failure_line(lower: &str) -> bool {
    lower.starts_with("assertion failed")
        || lower.starts_with("assertionerror")
        || lower.starts_with("assertion error")
        || lower.starts_with("error: expect(")
        || lower.starts_with("error: expect ")
}

fn playwright_failure_line(lower: &str) -> bool {
    let mut words = lower.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    first.chars().all(|character| character.is_ascii_digit()) && words.next() == Some("failed")
}
