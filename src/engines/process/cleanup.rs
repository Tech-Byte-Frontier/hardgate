#[cfg(unix)]
use std::io::{self, Read};
use std::process::{Child, ExitStatus};
#[cfg(unix)]
use std::process::{ChildStderr, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const TERMINATION_GRACE: Duration = Duration::from_millis(200);
const GROUP_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(unix)]
const GROUP_PROBE_GRACE: Duration = Duration::from_secs(2);
#[cfg(unix)]
const EXTERNAL_COMMAND_GRACE: Duration = Duration::from_secs(1);
#[cfg(unix)]
const EXTERNAL_DIAGNOSTIC_BYTES: usize = 4096;

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
    let target = format!("-{pid}");
    let output = run_bounded_kill(&[format!("-{signal}"), "--".to_string(), target], "kill")?;
    classify_kill_output(output)
}

#[cfg(unix)]
fn probe_process_group(pid: u32) -> Result<ProcessGroupState, String> {
    let target = format!("-{pid}");
    let output = run_bounded_kill(&["-0".to_string(), "--".to_string(), target], "kill -0")?;
    if !output.stderr_complete {
        return Err(
            "kill -0 diagnostic pipe did not close within the bounded cleanup window".to_string(),
        );
    }
    if no_such_process(&output.stderr) {
        return Ok(ProcessGroupState::Absent);
    }
    if output.status.success() && output.stderr.is_empty() {
        return Ok(ProcessGroupState::Present);
    }
    Err(format_kill_failure(
        "kill -0",
        output.status,
        &output.stderr,
        output.truncated,
    ))
}

#[cfg(unix)]
fn classify_kill_output(output: BoundedKillOutput) -> Result<SignalResult, String> {
    if !output.stderr_complete {
        Err("kill diagnostic pipe did not close within the bounded cleanup window".to_string())
    } else if no_such_process(&output.stderr) {
        Ok(SignalResult::Absent)
    } else if output.status.success() && output.stderr.is_empty() {
        Ok(SignalResult::Sent)
    } else {
        Err(format_kill_failure(
            "kill",
            output.status,
            &output.stderr,
            output.truncated,
        ))
    }
}

#[cfg(unix)]
struct BoundedKillOutput {
    status: ExitStatus,
    stderr: Vec<u8>,
    stderr_complete: bool,
    truncated: bool,
}

#[cfg(unix)]
fn run_bounded_kill(args: &[String], label: &str) -> Result<BoundedKillOutput, String> {
    let (mut process, mut stderr) = spawn_kill_helper(args, label)?;
    let mut capture = KillCapture::new();
    let status = poll_kill_helper(&mut process, &mut stderr, &mut capture, label)?;
    let stderr_complete = finish_kill_stderr(&mut stderr, &mut capture, label)?;
    Ok(BoundedKillOutput {
        status,
        stderr: capture.bytes,
        stderr_complete,
        truncated: capture.truncated,
    })
}

#[cfg(unix)]
fn spawn_kill_helper(args: &[String], label: &str) -> Result<(Child, ChildStderr), String> {
    use std::os::fd::AsRawFd;

    let mut command = Command::new("kill");
    command
        .args(args)
        .env("LC_ALL", "C")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut process = command
        .spawn()
        .map_err(|error| format!("could not execute {label}: {error}"))?;
    let Some(stderr) = process.stderr.take() else {
        return Err(abort_helper(
            &mut process,
            format!("{label} did not provide a diagnostic pipe"),
        ));
    };
    if let Err(error) = super::set_nonblocking(stderr.as_raw_fd()) {
        return Err(abort_helper(
            &mut process,
            format!("could not prepare {label} diagnostics: {error}"),
        ));
    }
    Ok((process, stderr))
}

#[cfg(unix)]
fn poll_kill_helper(
    process: &mut Child,
    stderr: &mut ChildStderr,
    capture: &mut KillCapture,
    label: &str,
) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + EXTERNAL_COMMAND_GRACE;
    loop {
        if let Some(status) = poll_kill_helper_once(process, stderr, capture, label)? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(abort_helper(
                process,
                format!("{label} helper did not exit within the bounded cleanup window"),
            ));
        }
        thread::sleep(GROUP_POLL_INTERVAL);
    }
}

#[cfg(unix)]
fn poll_kill_helper_once(
    process: &mut Child,
    stderr: &mut ChildStderr,
    capture: &mut KillCapture,
    label: &str,
) -> Result<Option<ExitStatus>, String> {
    if let Err(error) = capture.drain(stderr) {
        return Err(abort_helper(
            process,
            format!("could not read {label} diagnostics: {error}"),
        ));
    }
    match process.try_wait() {
        Ok(status) => Ok(status),
        Err(error) => Err(abort_helper(
            process,
            format!("failed to wait for {label} helper: {error}"),
        )),
    }
}

#[cfg(unix)]
fn finish_kill_stderr(
    stderr: &mut ChildStderr,
    capture: &mut KillCapture,
    label: &str,
) -> Result<bool, String> {
    let deadline = Instant::now() + TERMINATION_GRACE;
    loop {
        let complete = capture
            .drain(stderr)
            .map_err(|error| format!("could not read {label} diagnostics: {error}"))?;
        if complete || Instant::now() >= deadline {
            return Ok(complete);
        }
        thread::sleep(GROUP_POLL_INTERVAL);
    }
}

#[cfg(unix)]
struct KillCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

#[cfg(unix)]
impl KillCapture {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(EXTERNAL_DIAGNOSTIC_BYTES),
            truncated: false,
        }
    }

    fn drain(&mut self, stderr: &mut ChildStderr) -> io::Result<bool> {
        drain_pipe(stderr, &mut self.bytes, &mut self.truncated)
    }
}

#[cfg(unix)]
fn abort_helper(process: &mut Child, reason: String) -> String {
    let kill_error = process.kill().err();
    let wait_error = wait_for_direct_child(process, Instant::now() + TERMINATION_GRACE).err();
    let kill_detail = kill_error
        .map(|error| format!("; failed to terminate helper: {error}"))
        .unwrap_or_default();
    let wait_detail = wait_error
        .map(|error| format!("; {error}"))
        .unwrap_or_default();
    format!("{reason}{kill_detail}{wait_detail}")
}

#[cfg(unix)]
fn drain_pipe<R: Read>(
    reader: &mut R,
    bytes: &mut Vec<u8>,
    truncated: &mut bool,
) -> io::Result<bool> {
    let mut buffer = [0_u8; 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(read) => {
                let remaining = EXTERNAL_DIAGNOSTIC_BYTES.saturating_sub(bytes.len());
                let keep = remaining.min(read);
                bytes.extend_from_slice(&buffer[..keep]);
                *truncated |= keep < read;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn no_such_process(stderr: &[u8]) -> bool {
    String::from_utf8_lossy(stderr)
        .to_ascii_lowercase()
        .contains("no such process")
}

#[cfg(unix)]
fn format_kill_failure(
    command: &str,
    status: ExitStatus,
    stderr: &[u8],
    truncated: bool,
) -> String {
    let mut detail = String::from_utf8_lossy(stderr).trim().to_string();
    if truncated {
        detail.push_str("\n[kill diagnostic truncated]");
    }
    if detail.is_empty() {
        format!("{command} exited unsuccessfully ({status}) with no diagnostic")
    } else {
        format!("{command} exited unsuccessfully ({status}): {detail}")
    }
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
