use super::{
    CapturedOutput, Child, ChildPoll, CommandRoots, ProcessOutcome, ProcessWait, append_output,
    command_for_tokens, configure_process_group, finish_process_wait, poll_child, timeout_process,
    wait_error_process,
};
use crate::resources::{MutationGuard, check_pressure, managed::ManagedCommand};
use std::io;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

pub(super) fn run(tokens: &[String], roots: CommandRoots<'_>, timeout: Duration) -> ProcessOutcome {
    match execute(tokens, roots, timeout) {
        Ok(outcome) => outcome,
        Err(error) => ProcessOutcome::Failed {
            message: error.to_string(),
            output: String::new(),
        },
    }
}

fn execute(
    tokens: &[String],
    roots: CommandRoots<'_>,
    timeout: Duration,
) -> io::Result<ProcessOutcome> {
    let guard = MutationGuard::acquire()?;
    let mut command = command_for_tokens(tokens, roots, "mutation")?;
    // cargo-mutants serializes --in-place execution itself and rejects even
    // CARGO_MUTANTS_JOBS=1. Its private copy still inherits the OS boundary.
    if tokens.iter().any(|token| token == "--in-place") {
        command.env_remove("CARGO_MUTANTS_JOBS");
    }
    guard.budget.constrain_environment(&mut command);
    let inherited = crate::resources::runtime::inherited()?;
    let mut managed = if inherited {
        None
    } else {
        let managed = ManagedCommand::prepare(&mut command, guard.budget, timeout)?;
        if managed.is_none() {
            return Err(crate::resources::runtime::error(
                "mutation requires enforced CPU and memory limits; test command was not started",
            ));
        }
        managed
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command.spawn().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Failed to execute mutation command: {error}"),
        )
    })?;
    let mut captured = CapturedOutput::from_child(&mut child);
    let wait = wait_guarded(&mut child, timeout, &mut managed)
        .unwrap_or_else(|error| ProcessWait::Error(error.to_string()));
    let wait = finish_managed_wait(wait, &mut managed, &mut child);
    let outcome = finish_process_wait(wait, &mut child, &mut captured);
    Ok(describe(
        outcome,
        if inherited {
            "workload resource guard: inherited verified CPU, memory, swap and task limits".into()
        } else {
            guard.budget.description(managed.is_some())
        },
    ))
}

fn wait_guarded(
    child: &mut Child,
    timeout: Duration,
    managed: &mut Option<ManagedCommand>,
) -> io::Result<ProcessWait> {
    let start = Instant::now();
    loop {
        check_pressure()?;
        if let Some(status) = poll_guarded(child, managed)? {
            return Ok(ProcessWait::Exited(status));
        }
        if managed.as_ref().map_or_else(
            || start.elapsed() >= timeout,
            |managed| managed.timed_out(start, timeout),
        ) {
            return Ok(ProcessWait::Timeout);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn poll_guarded(
    child: &mut Child,
    managed: &mut Option<ManagedCommand>,
) -> io::Result<Option<std::process::ExitStatus>> {
    let status = match poll_child(child) {
        ChildPoll::Exited(status) => Some(status),
        ChildPoll::Running => None,
        ChildPoll::Error(error) => return Err(error),
    };
    match managed {
        Some(managed) => managed.poll(status),
        None => Ok(status),
    }
}

fn finish_managed_wait(
    wait: ProcessWait,
    managed: &mut Option<ManagedCommand>,
    child: &mut Child,
) -> ProcessWait {
    if let Some(managed) = managed
        && let Err(error) = managed.stop()
    {
        return wait_error_process(child, "mutation", error);
    }
    match wait {
        ProcessWait::Exited(status) => ProcessWait::Exited(status),
        ProcessWait::Timeout => timeout_process(child, "mutation"),
        ProcessWait::Error(message) => {
            wait_error_process(child, "mutation", io::Error::other(message))
        }
    }
}

fn describe(outcome: ProcessOutcome, description: String) -> ProcessOutcome {
    match outcome {
        ProcessOutcome::Completed { status, output } => ProcessOutcome::Completed {
            status,
            output: append_output(output, description),
        },
        ProcessOutcome::TimedOut { output } => ProcessOutcome::TimedOut {
            output: append_output(output, description),
        },
        ProcessOutcome::Failed { message, output } => ProcessOutcome::Failed {
            message,
            output: append_output(output, description),
        },
    }
}
