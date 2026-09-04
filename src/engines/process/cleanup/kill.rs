use std::io::{self, Read};
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::thread;
use std::time::Instant;

use super::{ProcessGroupState, SignalResult};

pub(super) fn signal_process_group(signal: &str, pid: u32) -> Result<SignalResult, String> {
    let target = format!("-{pid}");
    let output = run_bounded_kill(&[format!("-{signal}"), "--".to_string(), target], "kill")?;
    classify_kill_output(output)
}

pub(super) fn probe_process_group(pid: u32) -> Result<ProcessGroupState, String> {
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

pub(super) struct BoundedKillOutput {
    status: ExitStatus,
    stderr: Vec<u8>,
    stderr_complete: bool,
    truncated: bool,
}

fn run_bounded_kill(args: &[String], label: &str) -> Result<BoundedKillOutput, String> {
    run_bounded_kill_program("kill", args, label)
}

pub(super) fn run_bounded_kill_program(
    program: &str,
    args: &[String],
    label: &str,
) -> Result<BoundedKillOutput, String> {
    let (mut process, mut stderr) = spawn_kill_helper(program, args, label)?;
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

fn spawn_kill_helper(
    program: &str,
    args: &[String],
    label: &str,
) -> Result<(Child, ChildStderr), String> {
    use std::os::fd::AsRawFd;

    let mut command = Command::new(program);
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
    if let Err(error) = super::super::set_nonblocking(stderr.as_raw_fd()) {
        return Err(abort_helper(
            &mut process,
            format!("could not prepare {label} diagnostics: {error}"),
        ));
    }
    Ok((process, stderr))
}

fn poll_kill_helper(
    process: &mut Child,
    stderr: &mut ChildStderr,
    capture: &mut KillCapture,
    label: &str,
) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + super::EXTERNAL_COMMAND_GRACE;
    let mut poll = KillPoll {
        process,
        stderr,
        capture,
        label,
        deadline,
    };
    loop {
        if let Some(status) = poll_kill_helper_once(&mut poll)? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(abort_helper(
                poll.process,
                format!("{label} helper did not exit within the bounded cleanup window"),
            ));
        }
        thread::sleep(super::GROUP_POLL_INTERVAL);
    }
}

struct KillPoll<'a> {
    process: &'a mut Child,
    stderr: &'a mut ChildStderr,
    capture: &'a mut KillCapture,
    label: &'a str,
    deadline: Instant,
}

fn poll_kill_helper_once(poll: &mut KillPoll<'_>) -> Result<Option<ExitStatus>, String> {
    match poll.capture.drain(poll.stderr, poll.deadline) {
        Ok(DrainOutcome::Complete | DrainOutcome::Pending | DrainOutcome::Progress) => {}
        Ok(DrainOutcome::Exhausted) => {
            return Err(abort_helper(
                poll.process,
                format!(
                    "{} diagnostics exceeded the bounded drain budget; helper was aborted",
                    poll.label
                ),
            ));
        }
        Err(error) => {
            return Err(abort_helper(
                poll.process,
                format!("could not read {} diagnostics: {error}", poll.label),
            ));
        }
    }
    match poll.process.try_wait() {
        Ok(status) => Ok(status),
        Err(error) => Err(abort_helper(
            poll.process,
            format!("failed to wait for {} helper: {error}", poll.label),
        )),
    }
}

fn finish_kill_stderr(
    stderr: &mut ChildStderr,
    capture: &mut KillCapture,
    label: &str,
) -> Result<bool, String> {
    let deadline = Instant::now() + super::TERMINATION_GRACE;
    loop {
        let drained = capture
            .drain(stderr, deadline)
            .map_err(|error| format!("could not read {label} diagnostics: {error}"))?;
        match drained {
            DrainOutcome::Complete => return Ok(true),
            DrainOutcome::Exhausted => return Ok(false),
            DrainOutcome::Pending if Instant::now() >= deadline => return Ok(false),
            DrainOutcome::Pending | DrainOutcome::Progress => {}
        }
        thread::sleep(super::GROUP_POLL_INTERVAL);
    }
}

struct KillCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

impl KillCapture {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(super::EXTERNAL_DIAGNOSTIC_BYTES),
            truncated: false,
        }
    }

    fn drain(&mut self, stderr: &mut ChildStderr, deadline: Instant) -> io::Result<DrainOutcome> {
        drain_pipe(stderr, &mut self.bytes, &mut self.truncated, deadline)
    }
}

fn abort_helper(process: &mut Child, reason: String) -> String {
    let kill_error = process.kill().err();
    let wait_error =
        super::wait_for_direct_child(process, Instant::now() + super::TERMINATION_GRACE).err();
    let kill_detail = kill_error
        .map(|error| format!("; failed to terminate helper: {error}"))
        .unwrap_or_default();
    let wait_detail = wait_error
        .map(|error| format!("; {error}"))
        .unwrap_or_default();
    format!("{reason}{kill_detail}{wait_detail}")
}

fn drain_pipe<R: Read>(
    reader: &mut R,
    bytes: &mut Vec<u8>,
    truncated: &mut bool,
    deadline: Instant,
) -> io::Result<DrainOutcome> {
    let mut buffer = [0_u8; 1024];
    let mut state = DrainState {
        bytes,
        truncated,
        drained: 0,
    };
    for _ in 0..super::EXTERNAL_DRAIN_ITERATIONS {
        if Instant::now() >= deadline {
            *state.truncated = true;
            return Ok(DrainOutcome::Exhausted);
        }
        match drain_once(&mut *reader, &mut buffer, &mut state)? {
            DrainOutcome::Progress => {}
            result => return Ok(result),
        }
    }
    *state.truncated = true;
    Ok(DrainOutcome::Exhausted)
}

enum DrainOutcome {
    Complete,
    Pending,
    Exhausted,
    Progress,
}

struct DrainState<'a> {
    bytes: &'a mut Vec<u8>,
    truncated: &'a mut bool,
    drained: usize,
}

fn drain_once<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    state: &mut DrainState<'_>,
) -> io::Result<DrainOutcome> {
    match reader.read(buffer) {
        Ok(0) => Ok(DrainOutcome::Complete),
        Ok(read) => {
            if state.record(&buffer[..read]) {
                Ok(DrainOutcome::Exhausted)
            } else {
                Ok(DrainOutcome::Progress)
            }
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(DrainOutcome::Pending),
        Err(error) => Err(error),
    }
}

impl DrainState<'_> {
    fn record(&mut self, chunk: &[u8]) -> bool {
        let remaining = super::EXTERNAL_DIAGNOSTIC_BYTES.saturating_sub(self.bytes.len());
        let keep = remaining.min(chunk.len());
        self.bytes.extend_from_slice(&chunk[..keep]);
        *self.truncated |= keep < chunk.len();
        self.drained = self.drained.saturating_add(chunk.len());
        if self.drained >= super::EXTERNAL_DIAGNOSTIC_BYTES {
            *self.truncated = true;
            true
        } else {
            false
        }
    }
}

fn no_such_process(stderr: &[u8]) -> bool {
    String::from_utf8_lossy(stderr)
        .to_ascii_lowercase()
        .contains("no such process")
}

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
