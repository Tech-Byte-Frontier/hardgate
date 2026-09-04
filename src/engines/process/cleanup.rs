use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
#[path = "cleanup/kill.rs"]
mod kill;

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;

const TERMINATION_GRACE: Duration = Duration::from_millis(200);
const GROUP_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(unix)]
const GROUP_PROBE_GRACE: Duration = Duration::from_secs(2);
#[cfg(unix)]
const EXTERNAL_COMMAND_GRACE: Duration = Duration::from_secs(1);
#[cfg(unix)]
const EXTERNAL_DIAGNOSTIC_BYTES: usize = 4096;
#[cfg(unix)]
const EXTERNAL_DRAIN_ITERATIONS: usize = 16;

#[cfg(unix)]
pub(crate) fn timeout_scope() -> &'static str {
    "process group"
}

#[cfg(not(unix))]
pub(crate) fn timeout_scope() -> &'static str {
    "direct child"
}

#[cfg(unix)]
pub(super) fn terminate_process_tree(child: &mut Child) -> Result<(), String> {
    terminate_unix_process_group(child)
}

#[cfg(not(unix))]
pub(super) fn terminate_process_tree(child: &mut Child) -> Result<(), String> {
    terminate_direct_child(child)
}

#[cfg(unix)]
fn terminate_unix_process_group(child: &mut Child) -> Result<(), String> {
    let pid = child.id();
    let mut errors = Vec::new();
    let mut child_status = None;

    record_signal_result(&mut errors, "TERM", signal_process_group("TERM", pid));
    let group_present = wait_for_group_or_grace(pid, child, &mut child_status, &mut errors);
    let kill_result = group_present.then(|| signal_process_group("KILL", pid));
    if let Some(result) = &kill_result {
        record_signal_result(&mut errors, "KILL", clone_signal_result(result));
    }

    if child_status.is_none() {
        match reap_direct_child(child, kill_result.as_ref()) {
            Ok(status) => child_status = Some(status),
            Err(error) => errors.push(error),
        }
    }

    if let Err(error) = wait_for_group_absence(pid) {
        errors.push(error);
    }
    if errors.is_empty() && child_status.is_some() {
        Ok(())
    } else if errors.is_empty() {
        Err("timed-out process direct child did not report an exit status".to_string())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(unix)]
fn clone_signal_result(result: &Result<SignalResult, String>) -> Result<SignalResult, String> {
    match result {
        Ok(SignalResult::Sent) => Ok(SignalResult::Sent),
        Ok(SignalResult::Absent) => Ok(SignalResult::Absent),
        Err(error) => Err(error.clone()),
    }
}

#[cfg(unix)]
fn record_signal_result(
    errors: &mut Vec<String>,
    signal: &str,
    result: Result<SignalResult, String>,
) {
    if let Err(error) = result {
        errors.push(format!("failed to send SIG{signal}: {error}"));
    }
}

#[cfg(unix)]
fn wait_for_group_or_grace(
    pid: u32,
    child: &mut Child,
    child_status: &mut Option<ExitStatus>,
    errors: &mut Vec<String>,
) -> bool {
    let deadline = Instant::now() + TERMINATION_GRACE;
    loop {
        poll_direct_child(child, child_status, errors);
        match next_group_poll(pid, deadline) {
            GroupPoll::Absent => return false,
            GroupPoll::Expired => return true,
            GroupPoll::Continue => thread::sleep(GROUP_POLL_INTERVAL),
            GroupPoll::Error(error) => {
                errors.push(format!("failed to probe process group: {error}"));
                return true;
            }
        }
    }
}

#[cfg(unix)]
fn poll_direct_child(
    child: &mut Child,
    child_status: &mut Option<ExitStatus>,
    errors: &mut Vec<String>,
) {
    if child_status.is_some() {
        return;
    }
    match child.try_wait() {
        Ok(Some(status)) => *child_status = Some(status),
        Ok(None) => {}
        Err(error) => errors.push(format!("failed to wait for direct child: {error}")),
    }
}

#[cfg(unix)]
enum GroupPoll {
    Absent,
    Expired,
    Continue,
    Error(String),
}

#[cfg(unix)]
fn next_group_poll(pid: u32, deadline: Instant) -> GroupPoll {
    match probe_process_group(pid) {
        Ok(ProcessGroupState::Absent) => GroupPoll::Absent,
        Ok(ProcessGroupState::Present) if Instant::now() >= deadline => GroupPoll::Expired,
        Ok(ProcessGroupState::Present) => GroupPoll::Continue,
        Err(error) => GroupPoll::Error(error),
    }
}

#[cfg(unix)]
fn reap_direct_child(
    child: &mut Child,
    kill_result: Option<&Result<SignalResult, String>>,
) -> Result<ExitStatus, String> {
    if !matches!(kill_result, Some(Ok(SignalResult::Sent)))
        && let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed to wait for direct child: {error}"))?
    {
        return Ok(status);
    }
    force_reap_direct_child(child)
}

#[cfg(unix)]
fn force_reap_direct_child(child: &mut Child) -> Result<ExitStatus, String> {
    let kill_error = child.kill().err();
    let deadline = Instant::now() + TERMINATION_GRACE;
    match wait_for_direct_child(child, deadline) {
        Ok(status)
            if kill_error
                .as_ref()
                .is_none_or(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(status)
        }
        Ok(status) => Err(format!(
            "failed to terminate direct child after KILL: {}; direct child exited with {status}",
            kill_error.expect("kill error is present when reaching this branch")
        )),
        Err(wait_error) => {
            let kill_detail = kill_error
                .map(|error| format!("failed to terminate direct child after KILL: {error}; "))
                .unwrap_or_default();
            Err(format!("{kill_detail}{wait_error}"))
        }
    }
}

fn wait_for_direct_child(child: &mut Child, deadline: Instant) -> Result<ExitStatus, String> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                return Err("direct child remained running after bounded KILL grace".to_string());
            }
            Ok(None) => thread::sleep(GROUP_POLL_INTERVAL),
            Err(error) => return Err(format!("failed to wait for direct child: {error}")),
        }
    }
}

#[cfg(unix)]
fn wait_for_group_absence(pid: u32) -> Result<(), String> {
    let deadline = Instant::now() + GROUP_PROBE_GRACE;
    loop {
        match probe_process_group(pid)? {
            ProcessGroupState::Absent => return Ok(()),
            ProcessGroupState::Present if Instant::now() >= deadline => {
                return Err(format!(
                    "process group {pid} remained alive after termination"
                ));
            }
            ProcessGroupState::Present => thread::sleep(GROUP_POLL_INTERVAL),
        }
    }
}

#[cfg(unix)]
#[derive(Clone, Copy)]
enum ProcessGroupState {
    Present,
    Absent,
}

#[cfg(unix)]
enum SignalResult {
    Sent,
    Absent,
}

#[cfg(unix)]
fn signal_process_group(signal: &str, pid: u32) -> Result<SignalResult, String> {
    kill::signal_process_group(signal, pid)
}

#[cfg(unix)]
fn probe_process_group(pid: u32) -> Result<ProcessGroupState, String> {
    kill::probe_process_group(pid)
}

#[cfg(not(unix))]
fn terminate_direct_child(child: &mut Child) -> Result<(), String> {
    let kill_error = child.kill().err();
    let wait_error = wait_for_direct_child(child, Instant::now() + TERMINATION_GRACE).err();
    match (kill_error, wait_error) {
        (None, None) => Ok(()),
        (Some(error), None) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        (Some(kill), Some(wait)) => {
            Err(format!("failed to terminate direct child: {kill}; {wait}"))
        }
        (Some(kill), None) => Err(format!("failed to terminate direct child: {kill}")),
        (None, Some(wait)) => Err(wait),
    }
}
