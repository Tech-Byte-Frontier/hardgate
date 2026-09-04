use super::{fs, mutant, run_mutant_script, source_root};
use hardgate::engines::{BaselineOutcome, MutantOutcome, NativeMutationRunner};
use std::path::{Path, PathBuf};

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
fn playwright_failure_evidence_uses_framework_context() {
    let result = run_mutant_script(
        "mutation-runner-playwright-context",
        "sh -c \"printf 'playwright\\n\\n1 failed' >&2; exit 1\"".to_string(),
    );
    assert_eq!(result.outcome, MutantOutcome::Killed);
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
