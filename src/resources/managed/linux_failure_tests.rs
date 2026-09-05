use super::*;

pub(super) fn managed() -> ManagedCommand {
    ManagedCommand {
        controller: PathBuf::from("/bin/false"),
        high_bytes: 1024 * 1024,
        evidence: CommandEvidence::create(2 * 1024 * 1024).unwrap(),
        invocation: None,
        cgroup: None,
        cgroup_directory: None,
        last_poll: None,
        owned: false,
        started: None,
    }
}

fn group_fixture(command: &mut ManagedCommand, populated: &str) -> PathBuf {
    let path = command.evidence.shim_path().parent().unwrap().join(UNIT);
    fs::create_dir(&path).unwrap();
    fs::write(
        path.join("cgroup.events"),
        format!("populated {populated}\n"),
    )
    .unwrap();
    command.pin_cgroup(&path).unwrap();
    command.cgroup = Some(path.clone());
    path
}

#[test]
fn malformed_or_foreign_unit_status_never_establishes_ownership() {
    for output in ["", "invalid", "ActiveState=active\n"] {
        assert!(UnitStatus::parse(output).is_err());
    }
    for suffix in ["", " ", "foreign", " test"] {
        let output = format!("LoadState=loaded\nTransient=no\nDescription={DESCRIPTION}{suffix}\n");
        assert!(UnitStatus::parse(&output).unwrap().verify_owner().is_err());
    }
    for description in ["foreign", DESCRIPTION, &format!("{DESCRIPTION} ")] {
        let output = format!("LoadState=loaded\nTransient=yes\nDescription={description}\n");
        assert!(UnitStatus::parse(&output).unwrap().verify_owner().is_err());
    }
}

#[test]
fn missing_and_invalid_scope_identity_fields_are_distinct() {
    let empty = UnitStatus::parse("LoadState=loaded\n").unwrap();
    assert!(empty.invocation().unwrap().is_none());
    assert!(empty.cgroup().unwrap().is_none());
    for id in ["abc", "0123456789abcdef0123456789abcdeg00"] {
        let output = format!("LoadState=loaded\nInvocationID={id}\n");
        assert!(UnitStatus::parse(&output).unwrap().invocation().is_err());
    }
    assert!(
        !UnitStatus::parse("LoadState=not-found\n")
            .unwrap()
            .running()
    );
    assert!(
        !UnitStatus::parse("LoadState=loaded\nActiveState=active\nSubState=exited\n")
            .unwrap()
            .running()
    );
}

#[test]
fn pinning_handles_missing_paths_and_preserves_the_original_directory() {
    let mut command = managed();
    let root = command.evidence.shim_path().parent().unwrap().to_owned();
    command.pin_cgroup(&root.join("missing")).unwrap();
    assert!(command.cgroup_directory.is_none());
    assert!(
        command
            .pin_cgroup(&command.evidence.shim_path().join("child"))
            .is_err()
    );
    let group = group_fixture(&mut command, "0");
    command.pin_cgroup(&root).unwrap();
    assert_eq!(
        fs::read_link(format!(
            "/proc/self/fd/{}",
            command.cgroup_directory.as_ref().unwrap().as_raw_fd()
        ))
        .unwrap(),
        group
    );
}

#[test]
fn start_marker_requires_both_a_pinned_group_and_an_invocation() {
    let mut command = managed();
    command.start_when_pinned().unwrap();
    assert!(command.started.is_none());
    group_fixture(&mut command, "0");
    command.start_when_pinned().unwrap();
    assert!(command.started.is_none());
    command.invocation = Some("0123456789abcdef0123456789abcdef".to_owned());
    command.start_when_pinned().unwrap();
    let started = command.started.unwrap();
    command.start_when_pinned().unwrap();
    assert_eq!(command.started, Some(started));
}

#[test]
fn live_counter_loss_is_only_tolerated_for_a_proven_empty_group() {
    let mut command = managed();
    assert!(command.check_current_cgroup().is_ok());
    let group = group_fixture(&mut command, "1");
    assert!(command.check_current_cgroup().is_err());
    fs::write(group.join("cgroup.events"), "populated 0\n").unwrap();
    assert!(command.check_current_cgroup().is_ok());
    fs::write(group.join("memory.peak"), "invalid\n").unwrap();
    assert!(command.check_current_cgroup().is_err());
    fs::remove_file(group.join("memory.peak")).unwrap();
    fs::remove_file(group.join("cgroup.events")).unwrap();
    fs::remove_dir(&group).unwrap();
    assert!(command.check_current_cgroup().is_ok());
}

#[test]
fn cleanup_rejects_malformed_or_oversized_population_evidence() {
    let mut command = managed();
    let group = group_fixture(&mut command, "invalid");
    let directory = command.cgroup_directory.as_ref().unwrap();
    assert!(pinned_cgroup_populated(directory).is_err());
    fs::write(group.join("cgroup.events"), "x".repeat(1025)).unwrap();
    assert!(pinned_cgroup_populated(directory).is_err());
}

#[test]
fn pinned_cleanup_requires_a_kill_file_and_confirmed_absence() {
    let mut command = managed();
    let group = group_fixture(&mut command, "1");
    let directory = command.cgroup_directory.as_ref().unwrap();
    assert!(stop_pinned_cgroup(directory).is_err());
    fs::write(group.join("cgroup.kill"), "").unwrap();
    std::thread::scope(|scope| {
        let observed = scope.spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while fs::read(group.join("cgroup.kill")).unwrap() != b"1" {
                assert!(
                    Instant::now() < deadline,
                    "cleanup never requested cgroup.kill"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            fs::write(group.join("cgroup.events"), "populated 0\n").unwrap();
        });
        let result = stop_pinned_cgroup(directory);
        observed.join().unwrap();
        result.unwrap();
    });
    assert!(!pinned_cgroup_populated(directory).unwrap());
}

#[test]
fn pinned_cleanup_never_reports_success_for_a_group_that_stays_populated() {
    let mut command = managed();
    let group = group_fixture(&mut command, "1");
    fs::write(group.join("cgroup.kill"), "").unwrap();
    let error = stop_pinned_cgroup(command.cgroup_directory.as_ref().unwrap()).unwrap_err();
    assert!(error.to_string().contains("did not become empty"));
    assert_eq!(fs::read(group.join("cgroup.kill")).unwrap(), b"1");
}

#[test]
fn fallback_cleanup_preserves_the_original_controller_failure() {
    let mut command = managed();
    group_fixture(&mut command, "0");
    command.owned = true;
    let error = command.stop().unwrap_err();
    assert!(error.to_string().contains("systemd control command failed"));
    assert!(!command.owned);
    command.stop().unwrap();
}

#[test]
fn fallback_cleanup_reports_both_errors_without_false_success() {
    let mut command = managed();
    group_fixture(&mut command, "invalid");
    command.owned = true;
    let result = command.stop();
    command.owned = false;
    let error = result.unwrap_err().to_string();
    assert!(error.contains("systemd control command failed"));
    assert!(error.contains("cleanup failed"));
}

#[test]
fn control_failures_distinguish_spawn_nonzero_and_timeout() {
    let spawn = control_output(&["/hardgate-control-command-does-not-exist".into()]).unwrap_err();
    assert!(spawn.to_string().contains("Failed to execute"));
    let nonzero = control_output(&["/bin/false".into()]).unwrap_err();
    assert!(
        nonzero
            .to_string()
            .contains("systemd control command failed")
    );
    let timeout = control_output(&["/bin/sleep".into(), "5".into()]).unwrap_err();
    assert!(
        timeout
            .to_string()
            .contains("systemd control command timed out")
    );
}
