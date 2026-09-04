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

#[cfg(unix)]
fn timeout_fixture(tag: &str, script_name: &str, body: String) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let root = source_root(tag, b"true\n");
    let script = root.join(script_name);
    let pid_file = root.join("child.pid");
    let body = body.replace("{pid_file}", &pid_file.display().to_string());
    std::fs::write(&script, body).unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    (root, pid_file)
}

#[cfg(unix)]
fn assert_process_absent(pid: i32, context: &str) {
    for _ in 0..100 {
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !alive {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("{context} left descendant process {pid} alive");
}

#[cfg(unix)]
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

#[cfg(unix)]
struct TimeoutFixtureSpec<'a> {
    tag: &'a str,
    script_name: &'a str,
    body: &'a str,
    command: &'a str,
    context: &'a str,
    diagnostic: Option<&'a str>,
}

#[cfg(unix)]
fn run_timeout_fixture(spec: TimeoutFixtureSpec<'_>) {
    let (root, pid_file) = timeout_fixture(spec.tag, spec.script_name, spec.body.to_string());
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
            "printf 'could not compile crate' >&2; exit 1",
            MutantOutcome::CompileError,
        ),
        (
            "printf 'error[E0308]: mismatched types' >&2; exit 1",
            MutantOutcome::CompileError,
        ),
        ("printf 'failed' >&2; exit 1", MutantOutcome::RunnerError),
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
#[test]
fn timeout_terminates_process_group_descendants() {
    run_timeout_fixture(TimeoutFixtureSpec {
        tag: "mutation-runner-timeout",
        script_name: "hang.sh",
        body: "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{pid_file}'\nwait \"$!\"\n",
        command: "sh hang.sh",
        context: "timeout",
        diagnostic: None,
    });
}

#[cfg(unix)]
#[test]
fn timeout_kills_term_ignoring_descendant_and_verifies_group_absence() {
    run_timeout_fixture(TimeoutFixtureSpec {
        tag: "mutation-runner-timeout-kill",
        script_name: "ignore-term.sh",
        body: "#!/bin/sh\ntrap '' TERM\nsleep 30 &\nprintf '%s' \"$!\" > '{pid_file}'\nwait\n",
        command: "sh ignore-term.sh",
        context: "TERM-ignoring timeout",
        diagnostic: Some("terminated and absence was verified"),
    });
}

#[cfg(unix)]
#[test]
fn restoration_refuses_symlink_sabotage_without_touching_external_file() {
    let root = source_root("mutation-runner-restore-symlink", b"true\n");
    let target = root.join("fixture.rs");
    let outside = root.join("outside.txt");
    std::fs::write(&outside, b"outside\n").unwrap();
    let result = run_mutant_at_root(
        &root,
        "sh -c 'rm -f fixture.rs; ln -s outside.txt fixture.rs'".to_string(),
    );
    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(!result.source_restored);
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside\n");
    assert!(
        std::fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn restoration_refuses_directory_sabotage() {
    let root = source_root("mutation-runner-restore-directory", b"true\n");
    let target = root.join("fixture.rs");
    let result = run_mutant_at_root(
        &root,
        "sh -c 'rm -f fixture.rs; mkdir fixture.rs'".to_string(),
    );
    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(!result.source_restored);
    assert!(target.is_dir());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
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

#[cfg(unix)]
#[test]
fn atomic_mutation_replacement_detaches_hardlink_without_touching_peer() {
    let root = source_root("mutation-runner-hardlink", b"true\n");
    let target = root.join("fixture.rs");
    let peer = root.join("peer.rs");
    std::fs::hard_link(&target, &peer).unwrap();
    let result = run_mutant_at_root(&root, "printf executed".to_string());
    assert_eq!(result.outcome, MutantOutcome::Survived);
    assert!(result.source_restored);
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    assert_eq!(std::fs::read(&peer).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}
