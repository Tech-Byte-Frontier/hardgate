use super::*;

#[test]
fn relative_path_entries_resolve_in_the_test_commands_working_directory() {
    let evidence = CommandEvidence::create(1024 * 1024).unwrap();
    let root = evidence.shim_path().parent().unwrap();
    let bin = root.join("tools");
    fs::create_dir(&bin).unwrap();
    let executable = bin.join("local-runner");
    fs::write(&executable, "fixture").unwrap();
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
