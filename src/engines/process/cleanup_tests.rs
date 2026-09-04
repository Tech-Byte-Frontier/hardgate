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
            signal_process_group("USR1", pid).expect_err("unsupported signals must be rejected"),
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
            termination_result(Some(std::process::ExitStatus::from_raw(0)), Vec::new(),).is_ok()
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
