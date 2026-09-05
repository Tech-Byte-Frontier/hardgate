use super::launch::systemd_version;
use super::*;

fn unit(active: &str, substate: &str, result: &str, termination: &str) -> UnitStatus {
    UnitStatus::parse(&format!(
        "LoadState=loaded\nActiveState={active}\nSubState={substate}\nResult={result}\nDescription={DESCRIPTION} test-identity\nTransient=yes\nInvocationID=0123456789abcdef0123456789abcdef\n{termination}\n"
    )).unwrap()
}

#[test]
fn reserved_scope_collisions_are_refused_even_when_inactive() {
    let running = unit("active", "running", "success", "");
    assert!(running.running());
    assert!(reject_previous(&running).is_err());
    let inactive = unit("inactive", "dead", "success", "");
    assert!(!inactive.running());
    assert!(reject_previous(&inactive).is_err());
    let missing = UnitStatus::parse("LoadState=not-found\n").unwrap();
    assert!(reject_previous(&missing).is_ok());
    assert!(UnitStatus::parse("LoadState=loaded\nLoadState=not-found\n").is_err());
}

#[test]
fn wrapped_arguments_preserve_literals_and_keep_environment_values_out_of_argv() {
    let mut original = Command::new("/bin/sh");
    original.args(["-c", "printf '%s' '$HOME'", "argument with spaces"]);
    original.env("HARDGATE_RESOURCE_TEST_VALUE", "private test value");
    let budget = MutationBudget {
        memory_bytes: 1024 * 1024 * 1024,
        reserve_bytes: 0,
        jobs: 2,
    };
    let evidence = CommandEvidence::create(budget.memory_bytes).unwrap();
    let wrapped = wrap_command(
        &original,
        Path::new("/usr/bin/systemd-run"),
        LaunchSettings {
            budget,
            timeout: Duration::from_secs(2),
            evidence: &evidence,
        },
    )
    .unwrap();
    let arguments: Vec<_> = wrapped
        .get_args()
        .map(|value| value.to_string_lossy())
        .collect();
    assert!(
        arguments
            .iter()
            .any(|value| value == "--expand-environment=no")
    );
    assert!(
        arguments
            .iter()
            .any(|value| value == "--property=CPUQuota=200%")
    );
    assert!(arguments.iter().any(|value| value == "--scope"));
    assert!(
        wrapped
            .get_envs()
            .any(|(key, value)| key == "HARDGATE_RESOURCE_TEST_VALUE"
                && value == Some(std::ffi::OsStr::new("private test value")))
    );
    assert!(
        !arguments
            .iter()
            .any(|value| value.contains("private test value"))
    );
    assert_eq!(
        &arguments[arguments.len() - 4..],
        [
            "/bin/sh",
            "-c",
            "printf '%s' '$HOME'",
            "argument with spaces"
        ]
    );
}

#[test]
fn systemd_version_parser_rejects_ambiguous_inputs() {
    assert_eq!(systemd_version("systemd 255 (255.4)\nfeatures"), Some(255));
    assert_eq!(systemd_version("other 255"), None);
    assert_eq!(systemd_version("systemd future"), None);
}

#[test]
fn cleanup_requires_the_same_private_identity_and_invocation() {
    let status = unit(
        "active",
        "running",
        "success",
        "ExecMainCode=0\nExecMainStatus=0",
    );
    let invocation = "0123456789abcdef0123456789abcdef";
    assert!(
        status
            .verify_identity("test-identity", Some(invocation))
            .is_ok()
    );
    assert!(
        status
            .verify_identity("another-command", Some(invocation))
            .is_err()
    );
    assert!(
        status
            .verify_identity("test-identity", Some("ffffffffffffffffffffffffffffffff"))
            .is_err()
    );
    assert_eq!(status.invocation().unwrap(), Some(invocation));
}

#[test]
fn cgroup_cleanup_paths_cannot_escape_the_reserved_service() {
    for path in [
        "/user.slice/hardgate-native-mutation.scope",
        "/hardgate-native-mutation.scope",
    ] {
        let status =
            UnitStatus::parse(&format!("LoadState=loaded\nControlGroup={path}\n")).unwrap();
        assert!(
            status
                .cgroup()
                .unwrap()
                .unwrap()
                .starts_with("/sys/fs/cgroup")
        );
    }
    for path in [
        "relative/hardgate-native-mutation.scope",
        "/user.slice/../hardgate-native-mutation.scope",
        "/user.slice/other.service",
    ] {
        let status =
            UnitStatus::parse(&format!("LoadState=loaded\nControlGroup={path}\n")).unwrap();
        assert!(status.cgroup().is_err());
    }
}

#[test]
fn cleanup_distinguishes_removed_pinned_groups_from_missing_live_counters() {
    let fixture = CommandEvidence::create(1024 * 1024).unwrap();
    let path = fixture.shim_path().parent().unwrap().join(UNIT);
    fs::create_dir(&path).unwrap();
    let directory = fs::File::open(&path).unwrap();
    assert!(pinned_cgroup_populated(&directory).is_err());
    fs::write(path.join("cgroup.events"), "populated 1\n").unwrap();
    assert!(pinned_cgroup_populated(&directory).unwrap());
    fs::write(path.join("cgroup.events"), "populated 0\n").unwrap();
    assert!(!pinned_cgroup_populated(&directory).unwrap());
    fs::remove_file(path.join("cgroup.events")).unwrap();
    fs::remove_dir(&path).unwrap();
    assert!(!pinned_cgroup_populated(&directory).unwrap());
    // Reusing the old name must not redirect the pinned descriptor.
    fs::create_dir(&path).unwrap();
    fs::write(path.join("cgroup.events"), "populated 1\n").unwrap();
    assert!(!pinned_cgroup_populated(&directory).unwrap());
}

#[test]
fn scope_startup_has_its_own_bound_before_the_command_timeout() {
    let mut managed = super::failure_tests::managed();
    let launched = Instant::now() - Duration::from_secs(2);
    let timeout = Duration::from_secs(1);
    assert!(!managed.timed_out(launched, timeout));
    managed.started = Some(Instant::now());
    assert!(!managed.timed_out(launched, timeout));
    managed.started = Some(launched);
    assert!(managed.timed_out(launched, timeout));
    managed.started = None;
    assert!(managed.timed_out(Instant::now() - Duration::from_secs(7), timeout));
}
