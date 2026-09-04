#[path = "support/fs.rs"]
mod fs;

use hardgate::engines::{AstMutant, BaselineOutcome, MutantOutcome, NativeMutationRunner};
use std::path::{Path, PathBuf};

fn mutant(file: &str, start_byte: usize, end_byte: usize, original: &str) -> AstMutant {
    AstMutant {
        id: 1,
        file: PathBuf::from(file),
        line: 1,
        column: start_byte + 1,
        start_byte,
        end_byte,
        original: original.to_string(),
        replacement: "false".to_string(),
        description: "test mutant".to_string(),
    }
}

fn source_root(tag: &str, contents: &[u8]) -> PathBuf {
    let root = fs::tempdir(tag);
    std::fs::write(root.join("fixture.rs"), contents).unwrap();
    root
}

fn run_mutant_at_root(root: &Path, command: String) -> hardgate::engines::MutantExecutionResult {
    let runner = NativeMutationRunner::new(2, Some(command));
    runner.run_mutant(&mutant("fixture.rs", 0, 4, "true"), root)
}

fn run_mutant_script(tag: &str, command: String) -> hardgate::engines::MutantExecutionResult {
    let root = source_root(tag, b"true\n");
    let result = run_mutant_at_root(&root, command);
    assert!(result.source_restored);
    let _ = std::fs::remove_dir_all(root);
    result
}

fn run_baseline_script(tag: &str, script: &str) -> hardgate::engines::BaselineExecutionResult {
    let root = source_root(tag, b"fn main() {}\n");
    let runner = NativeMutationRunner::new(2, Some(format!("sh -c '{script}'")));
    let result = runner.run_baseline(Path::new("fixture.rs"), Path::new(&root));
    let _ = std::fs::remove_dir_all(root);
    result
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn timeout_fixture(tag: &str, script_name: &str, body: String) -> (PathBuf, PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let root = source_root(tag, b"true\n");
    let script = root.join(script_name);
    let pid_file = root.join("child.pid");
    let group_pid_file = root.join("group.pid");
    let body = body
        .replace("{pid_file}", &pid_file.display().to_string())
        .replace("{group_pid_file}", &group_pid_file.display().to_string());
    std::fs::write(&script, body).unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    (root, pid_file, group_pid_file)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn assert_process_absent(pid: i32, context: &str) {
    let pid = rustix::process::Pid::from_raw(pid).expect("fixture pid must be positive");
    for _ in 0..100 {
        match rustix::io::retry_on_intr(|| rustix::process::test_kill_process(pid)) {
            Err(error) if error == rustix::io::Errno::SRCH => return,
            Ok(()) => {}
            Err(error) => panic!("{context} process probe failed: {error}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("{context} left descendant process {pid} alive");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn assert_process_group_absent(pgid: i32, context: &str) {
    let pgid =
        rustix::process::Pid::from_raw(pgid).expect("fixture process-group id must be positive");
    for _ in 0..100 {
        match rustix::io::retry_on_intr(|| rustix::process::test_kill_process_group(pgid)) {
            Err(error) if error == rustix::io::Errno::SRCH => return,
            Ok(()) => {}
            Err(error) => panic!("{context} process-group probe failed: {error}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("{context} left process group {pgid} alive");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_executable(root: &Path, path: &str, content: &str) {
    use std::os::unix::fs::PermissionsExt;

    let target = root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&target, content).unwrap();
    let mut permissions = std::fs::metadata(&target).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(target, permissions).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct TimeoutFixtureSpec<'a> {
    tag: &'a str,
    script_name: &'a str,
    body: &'a str,
    command: &'a str,
    context: &'a str,
    diagnostic: Option<&'a str>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_timeout_fixture(spec: TimeoutFixtureSpec<'_>) {
    let (root, pid_file, group_pid_file) =
        timeout_fixture(spec.tag, spec.script_name, spec.body.to_string());
    let runner = NativeMutationRunner::new(1, Some(spec.command.to_string()));
    let result = runner.run_mutant(&mutant("fixture.rs", 0, 4, "true"), Path::new(&root));
    assert_eq!(result.outcome, MutantOutcome::Timeout);
    assert!(result.source_restored);
    if let Some(needle) = spec.diagnostic {
        assert!(result.diagnostic.contains(needle));
    }
    let child_pid = std::fs::read_to_string(pid_file).unwrap();
    let child_pid = child_pid.trim().parse::<i32>().unwrap();
    assert_process_absent(child_pid, spec.context);
    let group_pid = std::fs::read_to_string(group_pid_file).unwrap();
    let group_pid = group_pid.trim().parse::<i32>().unwrap();
    assert_process_group_absent(group_pid, spec.context);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_mutant_restores_original_bytes_and_reports_diagnostic() {
    let root = fs::tempdir("mutation-runner-restore");
    let target = root.join("fixture.rs");
    let original = b"true\n";
    std::fs::write(&target, original).unwrap();

    let runner = NativeMutationRunner::new(2, Some("sh -c 'printf test-output'".to_string()));
    let result = runner.run_mutant(&mutant("fixture.rs", 0, 4, "true"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::Survived);
    assert!(result.source_restored);
    assert_eq!(std::fs::read(&target).unwrap(), original);
    assert!(result.diagnostic.contains("test-output"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn baseline_distinguishes_failure_and_missing_command() {
    let root = fs::tempdir("mutation-runner-baseline");
    let target = root.join("fixture.rs");
    std::fs::write(&target, b"fn main() {}\n").unwrap();

    let failing = NativeMutationRunner::new(
        2,
        Some("sh -c 'printf baseline-failure >&2; exit 1'".to_string()),
    );
    let failure = failing.run_baseline(Path::new("fixture.rs"), Path::new(&root));
    assert_eq!(failure.outcome, BaselineOutcome::Failed);
    assert!(failure.diagnostic.contains("baseline-failure"));

    let missing =
        NativeMutationRunner::new(2, Some("hardgate-command-that-does-not-exist".to_string()));
    let missing_result = missing.run_baseline(Path::new("fixture.rs"), Path::new(&root));
    assert_eq!(missing_result.outcome, BaselineOutcome::RunnerError);
    assert!(missing_result.diagnostic.contains("Failed to execute"));
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_deleting_target_returns_runner_error_and_restores_exact_entry() {
    use std::os::unix::fs::PermissionsExt;

    let root = source_root("mutation-runner-baseline-restore", b"fn main() {}\n");
    let target = root.join("fixture.rs");
    let original = b"fn main() {}\n";
    let original_mode = 0o640;
    let mut permissions = std::fs::metadata(&target).unwrap().permissions();
    permissions.set_mode(original_mode);
    std::fs::set_permissions(&target, permissions).unwrap();

    let runner = NativeMutationRunner::new(2, Some("sh -c 'rm -f fixture.rs'".to_string()));
    let result = runner.run_baseline(Path::new("fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, BaselineOutcome::RunnerError);
    assert!(result.diagnostic.contains("modified source"));
    let metadata = std::fs::metadata(&target).unwrap();
    assert!(metadata.file_type().is_file());
    assert_eq!(std::fs::read(&target).unwrap(), original);
    assert_eq!(metadata.permissions().mode() & 0o7777, original_mode);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compile_diagnostic_is_not_misreported_as_a_killed_mutant() {
    let result = run_mutant_script(
        "mutation-runner-compile",
        "sh -c 'printf \"error[E0308]: mismatched types\\n\" >&2; exit 1'".to_string(),
    );
    assert_eq!(result.outcome, MutantOutcome::CompileError);
}

#[test]
fn mutation_outcomes_require_known_failure_evidence() {
    let cases = [
        (
            "printf 'test result: FAILED' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        (
            "printf 'thread case panicked at src/lib.rs:1' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        ("printf 'failures:' >&2; exit 1", MutantOutcome::Killed),
        (
            "printf 'FAIL tests/example.test.ts' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        (
            "printf 'Test Suites: 1 failed' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        (
            "printf 'Test Files 1 failed' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        (
            "printf 'AssertionError: expected true' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        (
            "printf 'not ok feature test' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        (
            "printf 'FAILED tests/test_feature.py' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        (
            "printf 'playwright\\n\\n1 failed' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        (
            "printf 'fail tests/test_feature.rs' >&2; exit 1",
            MutantOutcome::Killed,
        ),
        (
            "printf 'could not compile crate' >&2; exit 1",
            MutantOutcome::CompileError,
        ),
        (
            "printf 'error[E0308]: mismatched types' >&2; exit 1",
            MutantOutcome::CompileError,
        ),
        ("printf 'failed' >&2; exit 1", MutantOutcome::RunnerError),
        ("printf 'FAIL' >&2; exit 1", MutantOutcome::RunnerError),
        ("printf '1 failed' >&2; exit 1", MutantOutcome::RunnerError),
        ("printf 'panicked' >&2; exit 1", MutantOutcome::RunnerError),
        (
            "printf 'untrusted error[E9999] text' >&2; exit 1",
            MutantOutcome::RunnerError,
        ),
        ("printf 'error' >&2; exit 1", MutantOutcome::RunnerError),
        ("exit 1", MutantOutcome::RunnerError),
        ("exit 126", MutantOutcome::RunnerError),
    ];
    for (index, (script, expected)) in cases.into_iter().enumerate() {
        let result = run_mutant_script(
            &format!("mutation-runner-classification-{index}"),
            format!("sh -c \"{script}\""),
        );
        assert_eq!(result.outcome, expected, "script: {script}");
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn mutation_signal_is_a_runner_error() {
    let result = run_mutant_script(
        "mutation-runner-signal",
        "sh -c 'kill -TERM $$'".to_string(),
    );
    assert_eq!(result.outcome, MutantOutcome::RunnerError);
}

#[test]
fn baseline_classification_keeps_execution_context() {
    let cases = [
        ("exit 1", BaselineOutcome::Failed),
        ("printf failure >&2; exit 2", BaselineOutcome::Failed),
        ("exit 126", BaselineOutcome::RunnerError),
        ("exit 127", BaselineOutcome::RunnerError),
        ("exit 130", BaselineOutcome::RunnerError),
    ];
    for (index, (script, expected)) in cases.into_iter().enumerate() {
        let result =
            run_baseline_script(&format!("mutation-runner-baseline-context-{index}"), script);
        assert_eq!(result.outcome, expected, "script: {script}");
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_signal_and_timeout_are_not_ordinary_failures() {
    let signal_result = run_baseline_script("mutation-runner-baseline-signal", "kill -TERM $$");
    assert_eq!(signal_result.outcome, BaselineOutcome::RunnerError);

    let root = source_root("mutation-runner-baseline-timeout", b"fn main() {}\n");
    let timed_out = NativeMutationRunner::new(1, Some("sleep 30".to_string()));
    let timeout_result = timed_out.run_baseline(Path::new("fixture.rs"), Path::new(&root));
    assert_eq!(timeout_result.outcome, BaselineOutcome::Timeout);
    assert!(
        timeout_result
            .diagnostic
            .contains("terminated and absence was verified")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn stale_mutant_original_is_unviable_without_writing_or_running_command() {
    let root = source_root("mutation-runner-stale-original", b"true\n");
    let target = root.join("fixture.rs");
    let marker = root.join("executed");
    let command = format!("sh -c 'touch {}'", marker.display());
    let runner = NativeMutationRunner::new(2, Some(command));

    let result = runner.run_mutant(&mutant("fixture.rs", 0, 4, "false"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::Unviable);
    assert!(result.source_restored);
    assert!(!marker.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn missing_or_directory_target_returns_runner_error_without_running_command() {
    let missing_root = fs::tempdir("mutation-runner-missing-target");
    let missing_marker = missing_root.join("executed");
    let missing_runner = NativeMutationRunner::new(
        2,
        Some(format!("sh -c 'touch {}'", missing_marker.display())),
    );
    let missing_result = missing_runner.run_mutant(
        &mutant("missing.rs", 0, 4, "true"),
        Path::new(&missing_root),
    );
    assert_eq!(missing_result.outcome, MutantOutcome::RunnerError);
    assert!(!missing_result.source_restored);
    assert!(missing_result.diagnostic.contains("does not exist"));
    assert!(!missing_marker.exists());
    let _ = std::fs::remove_dir_all(missing_root);

    let directory_root = source_root("mutation-runner-directory-target", b"true\n");
    let directory_target = directory_root.join("fixture.rs");
    std::fs::remove_file(&directory_target).unwrap();
    std::fs::create_dir(&directory_target).unwrap();
    let directory_marker = directory_root.join("executed");
    let directory_runner = NativeMutationRunner::new(
        2,
        Some(format!("sh -c 'touch {}'", directory_marker.display())),
    );
    let directory_result = directory_runner.run_mutant(
        &mutant("fixture.rs", 0, 4, "true"),
        Path::new(&directory_root),
    );
    assert_eq!(directory_result.outcome, MutantOutcome::RunnerError);
    assert!(!directory_result.source_restored);
    assert!(directory_result.diagnostic.contains("not a regular file"));
    assert!(!directory_marker.exists());
    assert!(directory_target.is_dir());
    let _ = std::fs::remove_dir_all(directory_root);
}

#[test]
fn automatic_rust_and_unknown_plans_use_plain_commands_and_default_timeout() {
    let root = fs::tempdir("mutation-runner-automatic-plan");
    let root_path = root.as_path();
    let runner = NativeMutationRunner::new(2, None);

    assert_eq!(
        runner
            .resolve_test_command(Path::new("feature.rs"), root_path)
            .unwrap(),
        "cargo test feature"
    );
    for file in ["main.rs", "lib.rs", "mod.rs"] {
        assert_eq!(
            runner
                .resolve_test_command(Path::new(file), root_path)
                .unwrap(),
            "cargo test"
        );
    }
    assert_eq!(
        runner
            .resolve_test_command(Path::new("notes.txt"), root_path)
            .unwrap(),
        "cargo test"
    );
    assert_eq!(
        NativeMutationRunner::default_timeout_secs(&[PathBuf::from("notes.txt")], root_path, None,)
            .unwrap(),
        10
    );
    assert_eq!(
        NativeMutationRunner::default_timeout_secs(
            &[PathBuf::from("notes.txt")],
            root_path,
            Some("cargo test"),
        )
        .unwrap(),
        10
    );
    let _ = std::fs::remove_dir_all(root_path);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn whitespace_custom_command_is_runner_error_without_running_command() {
    let root = source_root("mutation-runner-whitespace-command", b"true\n");
    let target = root.join("fixture.rs");
    let runner = NativeMutationRunner::new(2, Some(" \t\n ".to_string()));
    let result = runner.run_mutant(&mutant("fixture.rs", 0, 4, "true"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(result.source_restored);
    assert!(
        result
            .diagnostic
            .contains("Empty command string; nothing was executed.")
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_runner_rejects_short_full_suite_timeout_before_execution() {
    let root = fs::tempdir("mutation-runner-full-suite-guard");
    std::fs::write(
        root.join("package.json"),
        br#"{"packageManager":"bun@1.1.0","scripts":{"test":"bun test"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("bun.lockb"), b"lock\n").unwrap();
    let source = root.join("src/feature.ts");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, b"true\n").unwrap();

    let runner = NativeMutationRunner::new(1, None);
    let baseline = runner.run_baseline(Path::new("src/feature.ts"), Path::new(&root));
    assert_eq!(baseline.outcome, BaselineOutcome::RunnerError);
    assert!(baseline.diagnostic.contains("timeout_secs >= 60"));

    let mutant = AstMutant {
        file: PathBuf::from("src/feature.ts"),
        ..mutant("fixture.rs", 0, 4, "true")
    };
    let result = runner.run_mutant(&mutant, Path::new(&root));
    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(result.source_restored);
    assert!(result.diagnostic.contains("timeout_secs >= 60"));
    assert_eq!(std::fs::read(source).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn javascript_runner_prefers_package_bin_and_falls_back_to_workspace_bin() {
    let root = fs::tempdir("mutation-runner-js-path");
    std::fs::write(
        root.join("package.json"),
        br#"{"private":true,"packageManager":"pnpm@9.0.0","workspaces":["packages/app"]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("pnpm-workspace.yaml"),
        b"packages:\n  - packages/app\n",
    )
    .unwrap();
    let package = root.join("packages/app");
    std::fs::create_dir_all(package.join("src")).unwrap();
    std::fs::write(
        package.join("package.json"),
        br#"{"name":"app","scripts":{"test":"vitest run"}}"#,
    )
    .unwrap();
    std::fs::write(package.join("src/feature.ts"), b"true\n").unwrap();
    std::fs::write(
        package.join("src/feature.test.ts"),
        b"test('feature', () => {});\n",
    )
    .unwrap();
    write_executable(
        &root,
        "node_modules/.bin/pnpm",
        "#!/bin/sh\nprintf workspace-bin\n",
    );
    write_executable(
        &root,
        "packages/app/node_modules/.bin/pnpm",
        "#!/bin/sh\nprintf package-bin\n",
    );

    let source = Path::new("packages/app/src/feature.ts");
    let runner = NativeMutationRunner::new(2, None);
    let package_result = runner.run_baseline(source, Path::new(&root));
    assert_eq!(package_result.outcome, BaselineOutcome::Passed);
    assert!(package_result.diagnostic.contains("package-bin"));
    assert!(!package_result.diagnostic.contains("workspace-bin"));

    std::fs::remove_file(package.join("node_modules/.bin/pnpm")).unwrap();
    let workspace_result = runner.run_baseline(source, Path::new(&root));
    assert_eq!(workspace_result.outcome, BaselineOutcome::Passed);
    assert!(workspace_result.diagnostic.contains("workspace-bin"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn equivalent_mutant_is_not_executed() {
    let root = source_root("mutation-runner-equivalent", b"true\n");
    let target = root.join("fixture.rs");
    let marker = root.join("executed");
    let equivalent = AstMutant {
        replacement: "true".to_string(),
        ..mutant("fixture.rs", 0, 4, "true")
    };
    let command = format!("sh -c 'touch {}'", marker.display());
    let runner = NativeMutationRunner::new(2, Some(command));
    let result = runner.run_mutant(&equivalent, Path::new(&root));
    assert_eq!(result.outcome, MutantOutcome::Equivalent);
    assert!(result.source_restored);
    assert!(!marker.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mutation_diagnostic_is_bounded_for_unknown_failure() {
    let result = run_mutant_script(
        "mutation-runner-bounded",
        "sh -c 'yes output | head -c 200000; exit 1'".to_string(),
    );
    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(result.diagnostic.len() <= 64 * 1024);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn timeout_terminates_process_group_descendants() {
    run_timeout_fixture(TimeoutFixtureSpec {
        tag: "mutation-runner-timeout",
        script_name: "hang.sh",
        body: "#!/bin/sh\nprintf '%s' \"$$\" > '{group_pid_file}'\nsleep 30 &\nprintf '%s' \"$!\" > '{pid_file}'\nwait \"$!\"\n",
        command: "sh hang.sh",
        context: "timeout",
        diagnostic: None,
    });
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn timeout_kills_term_ignoring_descendant_and_verifies_group_absence() {
    run_timeout_fixture(TimeoutFixtureSpec {
        tag: "mutation-runner-timeout-kill",
        script_name: "ignore-term.sh",
        body: "#!/bin/sh\nprintf '%s' \"$$\" > '{group_pid_file}'\ntrap '' TERM\nsleep 30 &\nprintf '%s' \"$!\" > '{pid_file}'\nwait\n",
        command: "sh ignore-term.sh",
        context: "TERM-ignoring timeout",
        diagnostic: Some("terminated and absence was verified"),
    });
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn restoration_recreates_deleted_target_with_original_bytes() {
    let root = source_root("mutation-runner-restore-delete", b"true\n");
    let target = root.join("fixture.rs");
    let result = run_mutant_at_root(
        &root,
        "sh -c 'rm -f fixture.rs; printf deleted'".to_string(),
    );
    assert_eq!(result.outcome, MutantOutcome::Survived);
    assert!(result.source_restored);
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn preexisting_hardlink_is_rejected_without_touching_peer() {
    let root = source_root("mutation-runner-hardlink", b"true\n");
    let target = root.join("fixture.rs");
    let peer = root.join("peer.rs");
    let marker = root.join("ran");
    std::fs::hard_link(&target, &peer).unwrap();
    let result = run_mutant_at_root(
        &root,
        format!("sh -c 'printf executed > {}'", marker.display()),
    );
    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(result.source_restored);
    assert!(!marker.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    assert_eq!(std::fs::read(&peer).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}
