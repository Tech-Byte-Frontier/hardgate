use super::runner::{BaselineOutcome, MutantOutcome};
use crate::engines::process::{
    CommandRoots, ProcessOutcome, append_output, run_command_with_roots, timeout_cleanup_evidence,
};
use std::process::ExitStatus;
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct CommandExecution {
    pub outcome: MutantOutcome,
    pub diagnostic: String,
    pub status: Option<ExitStatus>,
}

pub(crate) fn baseline_outcome(execution: &CommandExecution) -> BaselineOutcome {
    match execution.status.as_ref() {
        Some(status) if status.success() => BaselineOutcome::Passed,
        Some(status) if status_is_runner_error(status) => BaselineOutcome::RunnerError,
        Some(_) => BaselineOutcome::Failed,
        None if execution.outcome == MutantOutcome::Timeout => BaselineOutcome::Timeout,
        None => BaselineOutcome::RunnerError,
    }
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
                format!("Command timed out; {}", timeout_cleanup_evidence()),
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
    diagnostic.lines().any(compile_marker_line)
}

fn compile_marker_line(line: &str) -> bool {
    let line = line.trim().to_ascii_lowercase();
    [
        "could not compile",
        "error: could not compile",
        "compilation failed",
        "compile error",
        "compile_error",
        "syntaxerror",
        "syntax error",
        "failed to parse",
        "error[",
        "error ts",
        "typecheck failed",
        "tsc: error",
    ]
    .iter()
    .any(|marker| line.starts_with(marker))
}

fn looks_like_test_failure(diagnostic: &str) -> bool {
    let lower = diagnostic.to_ascii_lowercase();
    let framework_context = [
        "jest",
        "vitest",
        "playwright",
        "test suites",
        "test files",
        "test run",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    diagnostic
        .lines()
        .any(|line| test_failure_line(line, framework_context))
}

fn test_failure_line(line: &str, framework_context: bool) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    framework_failure_line(trimmed, &lower)
        || rust_failure_line(&lower)
        || assertion_failure_line(&lower)
        || playwright_failure_line(&lower, framework_context)
}

fn framework_failure_line(trimmed: &str, lower: &str) -> bool {
    [
        path_failure(trimmed),
        lower.starts_with("not ok "),
        pytest_failure(lower),
        suite_failure(lower),
    ]
    .into_iter()
    .any(|matched| matched)
}

fn path_failure(trimmed: &str) -> bool {
    let fail_path = trimmed
        .strip_prefix("FAIL")
        .or_else(|| trimmed.strip_prefix("fail"))
        .map(str::trim)
        .unwrap_or_default();
    !fail_path.is_empty()
        && ["/", "\\", ".", "test"]
            .iter()
            .any(|marker| fail_path.contains(marker))
}

fn pytest_failure(lower: &str) -> bool {
    lower.starts_with("failed ")
        && [".py", "/", "\\"]
            .iter()
            .any(|marker| lower.contains(marker))
}

fn suite_failure(lower: &str) -> bool {
    ["test suites:", "test files:", "test files "]
        .iter()
        .any(|prefix| lower.starts_with(prefix) && lower.contains(" failed"))
}

fn rust_failure_line(lower: &str) -> bool {
    lower.starts_with("failures:")
        || lower.starts_with("test result: failed")
        || lower.starts_with("test ") && lower.contains("... failed")
        || lower.starts_with("thread ") && lower.contains(" panicked at")
}

fn assertion_failure_line(lower: &str) -> bool {
    lower.starts_with("assertion failed")
        || lower.starts_with("assertionerror")
        || lower.starts_with("assertion error")
        || lower.starts_with("error: expect(")
        || lower.starts_with("error: expect ")
}

fn playwright_failure_line(lower: &str, framework_context: bool) -> bool {
    if !framework_context {
        return false;
    }
    let mut words = lower.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    first.chars().all(|character| character.is_ascii_digit()) && words.next() == Some("failed")
}
