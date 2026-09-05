use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn relative_path_entries_resolve_in_the_test_commands_working_directory() {
    let evidence = CommandEvidence::create(1024 * 1024).unwrap();
    let root = evidence.shim_path().parent().unwrap();
    let bin = root.join("tools");
    fs::create_dir(&bin).unwrap();
    let executable = bin.join("local-runner");
    fs::write(&executable, "fixture").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = Command::new("local-runner");
    command.current_dir(root).env("PATH", "missing:tools");
    assert_eq!(resolve_original_program(&command).unwrap(), executable);
}

#[test]
fn explicit_relative_executables_do_not_repeat_a_relative_working_directory() {
    let mut command = Command::new("./runner");
    command.current_dir("nested");
    assert_eq!(
        resolve_original_program(&command).unwrap(),
        std::env::current_dir().unwrap().join("nested/runner")
    );
    let command = Command::new("./runner");
    assert_eq!(
        resolve_original_program(&command).unwrap(),
        std::env::current_dir().unwrap().join("runner")
    );
}

#[test]
fn absolute_executables_and_environment_removals_survive_wrapping() {
    let mut command = Command::new("/bin/sh");
    command.env_remove("HARDGATE_REMOVED_FIXTURE_VALUE");
    let mut wrapped = Command::new("/unused-launcher");
    inherit_environment(&command, &mut wrapped);
    assert_eq!(
        resolve_original_program(&command).unwrap(),
        Path::new("/bin/sh")
    );
    assert!(
        wrapped
            .get_envs()
            .any(|(key, value)| key == "HARDGATE_REMOVED_FIXTURE_VALUE" && value.is_none())
    );
}

#[test]
fn empty_and_missing_search_paths_cannot_resolve_an_absent_executable() {
    let evidence = CommandEvidence::create(1024 * 1024).unwrap();
    let root = evidence.shim_path().parent().unwrap();
    let mut command = Command::new("hardgate-absent-test-runner");
    command.current_dir(root).env("PATH", "");
    assert!(
        resolve_original_program(&command)
            .unwrap_err()
            .to_string()
            .contains("Failed to execute")
    );
    command.env("PATH", "missing");
    assert!(resolve_original_program(&command).is_err());
}

#[test]
fn path_search_skips_non_executable_files_and_directories() {
    let evidence = CommandEvidence::create(1024 * 1024).unwrap();
    let root = evidence.shim_path().parent().unwrap();
    for directory in ["blocked", "directory", "usable"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    fs::write(root.join("blocked/runner"), "not executable").unwrap();
    fs::create_dir(root.join("directory/runner")).unwrap();
    let executable = root.join("usable/runner");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = Command::new("runner");
    command
        .current_dir(root)
        .env("PATH", "blocked:directory:usable");
    assert!(command.status().unwrap().success());
    assert_eq!(resolve_original_program(&command).unwrap(), executable);
}

#[test]
fn removing_path_uses_the_platform_default_search() {
    let mut command = Command::new("sh");
    command.env_remove("PATH");
    let expected = find_program(OsStr::new("sh"), Some(OsStr::new(DEFAULT_PATH)), None).unwrap();
    assert_eq!(resolve_original_program(&command).unwrap(), expected);
    command.arg("-c").arg("exit 0");
    assert!(command.status().unwrap().success());
    command.env("PATH", "");
    assert!(resolve_original_program(&command).is_err());
}

#[test]
fn removed_path_never_resolves_an_executable_only_in_the_parent_path() {
    const CHILD: &str = "HARDGATE_REMOVED_PATH_TEST_CHILD";
    const PROGRAM: &str = "hardgate-private-parent-path-runner";
    if std::env::var_os(CHILD).is_some() {
        let mut command = Command::new(PROGRAM);
        assert!(resolve_original_program(&command).is_ok());
        command.env_remove("PATH");
        assert!(resolve_original_program(&command).is_err());
        assert_eq!(
            command.status().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        return;
    }
    let evidence = CommandEvidence::create(1024 * 1024).unwrap();
    let root = evidence.shim_path().parent().unwrap();
    let executable = root.join(PROGRAM);
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("removed_path_never_resolves_an_executable_only_in_the_parent_path")
        .arg("--nocapture")
        .env(CHILD, "1")
        .env("PATH", root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[test]
fn tool_discovery_refuses_untrusted_runtime_and_missing_or_old_managers() {
    use std::os::unix::net::UnixListener;
    let evidence = CommandEvidence::create(1024 * 1024).unwrap();
    let root = evidence.shim_path().parent().unwrap();
    let uid = rustix::process::getuid().as_raw();
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let probe = |owner| available_tools_at(root, owner, Some(bin.as_os_str()));
    assert!(
        available_tools_at(&root.join("missing"), uid, Some(bin.as_os_str()))
            .unwrap()
            .is_none()
    );
    assert!(probe(uid).unwrap().is_none());
    fs::create_dir(root.join("systemd")).unwrap();
    let socket = root.join("systemd/private");
    fs::write(&socket, "not a socket").unwrap();
    assert!(probe(uid).unwrap().is_none());
    fs::remove_file(&socket).unwrap();
    let _listener = UnixListener::bind(&socket).unwrap();
    assert!(probe(uid.wrapping_add(1)).unwrap().is_none());
    fs::set_permissions(root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(probe(uid).unwrap().is_none());
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(probe(uid).unwrap().is_none());
    let launcher = bin.join("systemd-run");
    fs::write(&launcher, "#!/bin/sh\nprintf 'systemd 255\\n'\n").unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(probe(uid).unwrap().is_none());
    let controller = bin.join("systemctl");
    fs::write(&controller, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&controller, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(probe(uid).unwrap().unwrap(), (launcher.clone(), controller));
    for version in ["systemd 253", "unrecognized"] {
        fs::write(&launcher, format!("#!/bin/sh\nprintf '{version}\\n'\n")).unwrap();
        assert!(probe(uid).unwrap().is_none());
    }
    fs::write(&launcher, "#!/bin/sh\nexit 7\n").unwrap();
    assert!(probe(uid).is_err());
}
