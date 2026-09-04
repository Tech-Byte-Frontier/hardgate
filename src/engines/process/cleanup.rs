use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const TERMINATION_GRACE: Duration = Duration::from_millis(200);
const GROUP_PROBE_GRACE: Duration = Duration::from_secs(2);
const GROUP_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    match child.kill() {
        Ok(()) => child
            .wait()
            .map_err(|error| format!("failed to wait for direct child after KILL: {error}")),
        Err(kill_error) => child
            .try_wait()
            .map_err(|wait_error| {
                format!(
                    "failed to terminate direct child after KILL: {kill_error}; failed to wait: {wait_error}"
                )
            })?
            .ok_or_else(|| {
                format!(
                    "failed to terminate direct child after KILL: {kill_error}; direct child remained running"
                )
            }),
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
    let target = format!("-{pid}");
    let output = Command::new("kill")
        .args([format!("-{signal}"), "--".to_string(), target])
        .env("LC_ALL", "C")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("could not execute kill: {error}"))?;
    classify_kill_output(output.status, &output.stderr)
}

#[cfg(unix)]
fn probe_process_group(pid: u32) -> Result<ProcessGroupState, String> {
    let target = format!("-{pid}");
    let output = Command::new("kill")
        .args(["-0", "--", &target])
        .env("LC_ALL", "C")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("could not execute kill -0: {error}"))?;
    if output.status.success() {
        return Ok(ProcessGroupState::Present);
    }
    if no_such_process(&output.stderr) {
        Ok(ProcessGroupState::Absent)
    } else {
        Err(format_kill_failure(
            "kill -0",
            output.status,
            &output.stderr,
        ))
    }
}

#[cfg(unix)]
fn classify_kill_output(status: ExitStatus, stderr: &[u8]) -> Result<SignalResult, String> {
    if status.success() {
        Ok(SignalResult::Sent)
    } else if no_such_process(stderr) {
        Ok(SignalResult::Absent)
    } else {
        Err(format_kill_failure("kill", status, stderr))
    }
}

#[cfg(unix)]
fn no_such_process(stderr: &[u8]) -> bool {
    String::from_utf8_lossy(stderr)
        .to_ascii_lowercase()
        .contains("no such process")
}

#[cfg(unix)]
fn format_kill_failure(command: &str, status: ExitStatus, stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr).trim().to_string();
    if detail.is_empty() {
        format!("{command} exited unsuccessfully ({status}) with no diagnostic")
    } else {
        format!("{command} exited unsuccessfully ({status}): {detail}")
    }
}

#[cfg(not(unix))]
fn terminate_direct_child(child: &mut Child) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(error) = child.kill() {
        errors.push(format!("failed to terminate direct child: {error}"));
    }
    match child.wait() {
        Ok(_) => {}
        Err(error) => errors.push(format!("failed to wait for direct child: {error}")),
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
