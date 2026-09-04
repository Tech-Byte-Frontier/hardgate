use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

const TERMINATION_GRACE: Duration = Duration::from_millis(200);
const GROUP_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(any(target_os = "linux", target_os = "macos"))]
const GROUP_PROBE_GRACE: Duration = Duration::from_secs(2);

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn timeout_scope() -> &'static str {
    "process group"
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn timeout_scope() -> &'static str {
    "unavailable process cleanup"
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn timeout_cleanup_evidence() -> &'static str {
    "process group terminated and absence was verified."
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn timeout_cleanup_evidence() -> &'static str {
    "direct child terminated and was reaped; descendant cleanup is unavailable."
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn terminate_process_tree(child: &mut Child) -> Result<(), String> {
    terminate_unix_process_group(child)
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
pub(super) fn terminate_process_tree(_child: &mut Child) -> Result<(), String> {
    Err("process-group cleanup is unavailable outside Linux/macOS; mutation execution is unsupported".to_string())
}

#[cfg(not(unix))]
pub(super) fn terminate_process_tree(child: &mut Child) -> Result<(), String> {
    terminate_direct_child(child)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn terminate_unix_process_group(child: &mut Child) -> Result<(), String> {
    use rustix::process::Pid;

    let pid = Pid::from_child(child);
    validate_process_group_pid(pid)?;
    let mut errors = Vec::new();
    let mut child_status = None;

    record_signal_result(&mut errors, "TERM", signal_process_group("TERM", pid));
    let group_present = wait_for_group_or_grace(pid, child, &mut child_status, &mut errors);
    let kill_result = kill_remaining_group(group_present, pid, &mut errors);
    reap_after_group_signal(child, &mut child_status, kill_result.as_ref(), &mut errors);

    if let Err(error) = wait_for_group_absence(pid) {
        errors.push(error);
    }
    termination_result(child_status, errors)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_process_group_pid(pid: rustix::process::Pid) -> Result<(), String> {
    if pid.is_init() || pid.as_raw_pid() <= 1 {
        Err(format!(
            "refusing to signal process group for invalid PID {pid}"
        ))
    } else {
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn kill_remaining_group(
    group_present: bool,
    pid: rustix::process::Pid,
    errors: &mut Vec<String>,
) -> Option<Result<SignalResult, String>> {
    let result = group_present.then(|| signal_process_group("KILL", pid));
    if let Some(signal) = &result {
        record_signal_result(errors, "KILL", clone_signal_result(signal));
    }
    result
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn reap_after_group_signal(
    child: &mut Child,
    child_status: &mut Option<ExitStatus>,
    kill_result: Option<&Result<SignalResult, String>>,
    errors: &mut Vec<String>,
) {
    if child_status.is_some() {
        return;
    }
    match reap_direct_child(child, kill_result) {
        Ok(status) => *child_status = Some(status),
        Err(error) => errors.push(error),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn termination_result(child_status: Option<ExitStatus>, errors: Vec<String>) -> Result<(), String> {
    match (errors.is_empty(), child_status.is_some()) {
        (true, true) => Ok(()),
        (true, false) => {
            Err("timed-out process direct child did not report an exit status".to_string())
        }
        (false, _) => Err(errors.join("; ")),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn clone_signal_result(result: &Result<SignalResult, String>) -> Result<SignalResult, String> {
    match result {
        Ok(SignalResult::Sent) => Ok(SignalResult::Sent),
        Ok(SignalResult::Absent) => Ok(SignalResult::Absent),
        Err(error) => Err(error.clone()),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn record_signal_result(
    errors: &mut Vec<String>,
    signal: &str,
    result: Result<SignalResult, String>,
) {
    if let Err(error) = result {
        errors.push(format!("failed to send SIG{signal}: {error}"));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_for_group_or_grace(
    pid: rustix::process::Pid,
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
enum GroupPoll {
    Absent,
    Expired,
    Continue,
    Error(String),
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn next_group_poll(pid: rustix::process::Pid, deadline: Instant) -> GroupPoll {
    match probe_process_group(pid) {
        Ok(ProcessGroupState::Absent) => GroupPoll::Absent,
        Ok(ProcessGroupState::Present) if Instant::now() >= deadline => GroupPoll::Expired,
        Ok(ProcessGroupState::Present) => GroupPoll::Continue,
        Err(error) => GroupPoll::Error(error),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_for_group_absence(pid: rustix::process::Pid) -> Result<(), String> {
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy)]
enum ProcessGroupState {
    Present,
    Absent,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
enum SignalResult {
    Sent,
    Absent,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn signal_process_group(signal: &str, pid: rustix::process::Pid) -> Result<SignalResult, String> {
    use rustix::io::Errno;
    use rustix::process::{Signal, kill_process_group};

    if pid.as_raw_pid() <= 1 {
        return Err(format!(
            "refusing to signal process group for invalid PID {pid}"
        ));
    }
    let signal = match signal {
        "TERM" => Signal::TERM,
        "KILL" => Signal::KILL,
        other => return Err(format!("unsupported process-group signal {other}")),
    };
    match rustix::io::retry_on_intr(|| kill_process_group(pid, signal)) {
        Ok(()) => Ok(SignalResult::Sent),
        Err(error) if error == Errno::SRCH => Ok(SignalResult::Absent),
        Err(error) => Err(format!("kernel process-group signal failed: {error}")),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn probe_process_group(pid: rustix::process::Pid) -> Result<ProcessGroupState, String> {
    use rustix::io::Errno;
    use rustix::process::test_kill_process_group;

    if pid.as_raw_pid() <= 1 {
        return Err(format!(
            "refusing to probe process group for invalid PID {pid}"
        ));
    }
    match rustix::io::retry_on_intr(|| test_kill_process_group(pid)) {
        Ok(()) => Ok(ProcessGroupState::Present),
        Err(error) if error == Errno::SRCH => Ok(ProcessGroupState::Absent),
        Err(error) => Err(format!("kernel process-group probe failed: {error}")),
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

#[cfg(test)]
mod tests {
    use super::timeout_scope;

    #[test]
    fn timeout_scope_identifies_cleanup_strategy() {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert_eq!(timeout_scope(), "process group");
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        assert_eq!(timeout_scope(), "unavailable process cleanup");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    mod unix {
        use super::super::{
            GroupPoll, SignalResult, clone_signal_result, next_group_poll, probe_process_group,
            reap_direct_child, record_signal_result, signal_process_group, termination_result,
            validate_process_group_pid,
        };
        use rustix::process::Pid;
        use std::process::Command;
        use std::time::Instant;

        #[test]
        fn invalid_pid_is_rejected_before_group_syscalls() {
            let pid = Pid::from_raw(1).expect("PID 1 is nonzero");

            assert!(
                validate_process_group_pid(pid)
                    .expect_err("PID 1 must never be signaled")
                    .contains("invalid PID")
            );
            assert!(
                signal_process_group("TERM", pid)
                    .expect_err("PID 1 must never be signaled")
                    .contains("invalid PID")
            );
            assert!(
                probe_process_group(pid)
                    .expect_err("PID 1 must never be probed")
                    .contains("invalid PID")
            );
        }

        #[test]
        fn unsupported_signal_is_rejected_without_signaling() {
            let pid = Pid::from_raw(2).expect("PID 2 is nonzero");
            assert_eq!(
                signal_process_group("USR1", pid)
                    .expect_err("unsupported signals must be rejected"),
                "unsupported process-group signal USR1"
            );
        }

        #[test]
        fn signal_result_cloning_preserves_absence_and_errors() {
            assert!(matches!(
                clone_signal_result(&Ok(SignalResult::Sent)),
                Ok(SignalResult::Sent)
            ));
            assert!(matches!(
                clone_signal_result(&Ok(SignalResult::Absent)),
                Ok(SignalResult::Absent)
            ));

            let error = Err::<SignalResult, _>("kernel failure".to_string());
            assert!(matches!(
                clone_signal_result(&error),
                Err(message) if message == "kernel failure"
            ));
        }

        #[test]
        fn termination_result_reports_missing_status_and_aggregates_errors() {
            assert_eq!(
                termination_result(None, Vec::new()).expect_err("missing status must fail"),
                "timed-out process direct child did not report an exit status"
            );
            assert_eq!(
                termination_result(
                    None,
                    vec!["first failure".to_string(), "second failure".to_string()],
                )
                .expect_err("cleanup errors must fail"),
                "first failure; second failure"
            );

            use std::os::unix::process::ExitStatusExt;
            assert!(
                termination_result(Some(std::process::ExitStatus::from_raw(0)), Vec::new(),)
                    .is_ok()
            );
        }

        #[test]
        fn signal_errors_are_recorded_with_signal_name() {
            let mut errors = Vec::new();
            record_signal_result(&mut errors, "TERM", Err("kernel failure".to_string()));
            record_signal_result(&mut errors, "KILL", Ok(SignalResult::Absent));

            assert_eq!(errors, vec!["failed to send SIGTERM: kernel failure"]);
        }

        #[test]
        fn invalid_pid_poll_returns_group_error() {
            let pid = Pid::from_raw(1).expect("PID 1 is nonzero");
            assert!(matches!(
                next_group_poll(pid, Instant::now()),
                GroupPoll::Error(error) if error.contains("invalid PID")
            ));
        }

        #[test]
        fn reap_direct_child_handles_an_immediately_exited_child() {
            let mut child = Command::new("true")
                .spawn()
                .expect("true should be available in test environments");
            let expected = child.wait().expect("child should exit");
            let status = reap_direct_child(&mut child, None).expect("child should be reaped");

            assert_eq!(status, expected);
        }
    }
}
