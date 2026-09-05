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
        reap_direct_child, record_signal_result, signal_process_group, terminate_process_tree,
        termination_result, validate_process_group_pid, wait_for_direct_child,
        wait_for_group_absence,
    };
    use rustix::process::Pid;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    struct ChildGuard(Option<Child>);

    impl ChildGuard {
        fn new(child: Child) -> Self {
            Self(Some(child))
        }

        fn spawn(script: &str) -> Self {
            let child = Command::new("sh")
                .args(["-c", script])
                .stdin(Stdio::null())
                .process_group(0)
                .spawn()
                .expect("shell fixture should spawn");
            Self::new(child)
        }

        fn child(&mut self) -> &mut Child {
            self.0.as_mut().expect("child fixture is still owned")
        }

        fn pid(&mut self) -> Pid {
            Pid::from_child(self.child())
        }

        fn take(&mut self) -> Child {
            self.0.take().expect("child fixture is still owned")
        }

        fn disarm(&mut self) {
            let _ = self.0.take();
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = terminate_process_tree(child);
            }
        }
    }

    fn assert_error_contains<T>(result: Result<T, String>, expected: &str) {
        match result {
            Err(message) => assert!(message.contains(expected), "{message}"),
            Ok(_) => panic!("expected an error containing `{expected}`"),
        }
    }

    #[test]
    fn invalid_pid_is_rejected_before_group_syscalls() {
        let pid = Pid::from_raw(1).expect("PID 1 is nonzero");

        assert!(
            validate_process_group_pid(pid)
                .expect_err("PID 1 must never be signaled")
                .contains("invalid PID")
        );
        assert_error_contains(signal_process_group("TERM", pid), "invalid PID");
        assert_error_contains(probe_process_group(pid), "invalid PID");
    }

    #[test]
    fn unsupported_signal_is_rejected_without_signaling() {
        let pid = Pid::from_raw(2).expect("PID 2 is nonzero");
        assert_error_contains(
            signal_process_group("USR1", pid),
            "unsupported process-group signal USR1",
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

    #[test]
    fn reap_direct_child_forces_kill_after_group_kill() {
        let mut fixture = ChildGuard::spawn("sleep 1");
        let pid = fixture.pid();
        let kill_result = signal_process_group("KILL", pid);
        assert!(matches!(kill_result.as_ref(), Ok(SignalResult::Sent)));

        let status = reap_direct_child(fixture.child(), Some(&kill_result))
            .expect("group-killed child should be reaped");
        fixture.disarm();

        assert!(
            !status.success(),
            "KILL should produce a non-success status"
        );
    }

    #[test]
    fn wait_for_direct_child_rejects_a_running_child_after_deadline() {
        let mut fixture = ChildGuard::spawn("sleep 1");
        let result = wait_for_direct_child(fixture.child(), Instant::now());

        let error = result.expect_err("running child must exceed an immediate deadline");
        assert!(error.contains("remained running after bounded KILL grace"));
    }

    #[test]
    fn wait_for_direct_child_returns_a_completed_child() {
        let mut fixture = ChildGuard::spawn("true");
        let status =
            wait_for_direct_child(fixture.child(), Instant::now() + Duration::from_secs(1))
                .expect("completed child should be reaped");
        fixture.disarm();

        assert!(status.success());
    }

    #[test]
    fn process_group_poll_observes_present_and_absent_states() {
        let mut fixture = ChildGuard::spawn("sleep 1");
        let pid = fixture.pid();

        assert!(matches!(
            probe_process_group(pid),
            Ok(super::super::ProcessGroupState::Present)
        ));
        assert!(matches!(
            next_group_poll(pid, Instant::now() + Duration::from_secs(1)),
            GroupPoll::Continue
        ));
        assert!(matches!(
            next_group_poll(pid, Instant::now()),
            GroupPoll::Expired
        ));

        let signal = signal_process_group("TERM", pid).expect("owned group should be signaled");
        let status = fixture.child().wait().expect("group child should exit");
        fixture.disarm();
        assert!(matches!(signal, SignalResult::Sent));
        assert!(!status.success());
        assert!(matches!(
            probe_process_group(pid),
            Ok(super::super::ProcessGroupState::Absent)
        ));
        assert!(matches!(
            next_group_poll(pid, Instant::now() + Duration::from_secs(1)),
            GroupPoll::Absent
        ));
    }

    #[test]
    fn wait_for_group_absence_observes_a_running_group_then_reaped_absence() {
        let mut fixture = ChildGuard::spawn("sleep 0.2");
        let pid = fixture.pid();
        let barrier = Arc::new(Barrier::new(2));
        let thread_barrier = Arc::clone(&barrier);
        let child = fixture.take();
        let waiter = std::thread::spawn(move || {
            let mut fixture = ChildGuard::new(child);
            thread_barrier.wait();
            let status = fixture.child().wait().expect("group child should exit");
            fixture.disarm();
            status
        });

        barrier.wait();
        let absence = wait_for_group_absence(pid);
        let status = waiter.join().expect("child waiter should complete");

        assert!(absence.is_ok(), "group should become absent: {absence:?}");
        assert!(status.success());
        assert!(matches!(
            probe_process_group(pid),
            Ok(super::super::ProcessGroupState::Absent)
        ));
    }
}
