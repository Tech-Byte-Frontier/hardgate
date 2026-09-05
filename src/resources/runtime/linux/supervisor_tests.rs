use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::os::unix::process::ExitStatusExt;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixture() -> readiness::Readiness {
    readiness::Readiness::create(&format!(
        "supervisor-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
    .unwrap()
}

#[test]
fn manager_startup_status_never_claims_an_unacknowledged_evaluation() {
    let ready = fixture();
    for code in [0, 1, 2, 130, 143, 7] {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &format!("sleep 0.05; exit {code}")]);
        let result = supervise(command, &ready);
        if matches!(code, 2 | 130 | 143) {
            assert_eq!(result.unwrap(), code);
        } else {
            assert!(result.is_err(), "unacknowledged status {code}");
        }
    }
    assert!(supervise(Command::new("/hardgate-nonexistent-supervisor"), &ready).is_err());
    assert!(exit_code(std::process::ExitStatus::from_raw(9)).is_err());
}

#[test]
fn cancellation_reaps_an_unacknowledged_launcher() {
    let ready = fixture();
    let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    assert!(cancel(&mut child, &ready).is_err());
    assert!(child.try_wait().unwrap().is_some());
}

#[test]
fn launch_keeps_limits_and_literal_arguments_in_the_supervisor_command() {
    let ready = fixture();
    let runtime = ready.0.parent().unwrap();
    let command = launch(runtime, &ready.0, 1024 * 1024 * 1024).unwrap();
    let args = command
        .get_args()
        .map(|v| v.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    for value in [
        "--scope",
        "--collect",
        "--expand-environment=no",
        "--property=MemoryMax=1073741824",
        "--property=MemoryHigh=858993456",
        "--property=MemorySwapMax=0",
        "--property=TasksMax=256",
        "--property=RuntimeMaxSec=1800s",
    ] {
        assert!(args.iter().any(|arg| arg == value), "missing {value}");
    }
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == CHILD_MARKER && value == Some(ready.0.as_os_str()))
    );
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "DBUS_SESSION_BUS_ADDRESS" && value.is_none())
    );
    assert!(description(Path::new("/")).contains('/'));
}

#[test]
fn runtime_directory_requires_private_owner_and_a_real_manager_socket() {
    let ready = fixture();
    let root = ready.0.parent().unwrap();
    let uid = rustix::process::getuid().as_raw();
    assert!(validate_runtime(&root.join("missing"), uid).is_err());
    assert!(validate_runtime(root, uid).is_err());
    fs::create_dir(root.join("systemd")).unwrap();
    let socket = root.join("systemd/private");
    fs::write(&socket, "not a socket").unwrap();
    assert!(validate_runtime(root, uid).is_err());
    fs::remove_file(&socket).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    validate_runtime(root, uid).unwrap();
    assert!(validate_runtime(root, uid.wrapping_add(1)).is_err());
    fs::set_permissions(root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(validate_runtime(root, uid).is_err());
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    let link = root.with_extension("link");
    symlink(root, &link).unwrap();
    assert!(validate_runtime(&link, uid).is_err());
    fs::remove_file(link).unwrap();
    drop(listener);
    fs::remove_file(socket).unwrap();
    fs::remove_dir(root.join("systemd")).unwrap();
}

#[test]
fn cleanup_status_requires_matching_identity_and_known_lifecycle_states() {
    assert!(!scope_active("LoadState=not-found\n", "owned").unwrap());
    for state in ["inactive", "failed", "active", "activating", "deactivating"] {
        let output = format!("LoadState=loaded\nDescription=owned\nActiveState={state}\n");
        assert_eq!(
            scope_active(&output, "owned").unwrap(),
            matches!(state, "active" | "activating" | "deactivating")
        );
        assert!(scope_active(&output, "foreign").is_err());
    }
    assert!(scope_active("LoadState=loaded\nDescription=owned\n", "owned").is_err());
    assert!(scope_active("", "owned").is_err());
}
