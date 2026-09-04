#[path = "support/fs.rs"]
mod fs;

use hardgate::engines::{
    AstMutant, BaselineOutcome, MutantExecutionResult, MutantOutcome, NativeMutationRunner,
};
use std::path::{Path, PathBuf};

fn mutant(file: &str) -> AstMutant {
    AstMutant {
        id: 1,
        file: PathBuf::from(file),
        line: 1,
        column: 1,
        start_byte: 0,
        end_byte: 4,
        original: "true".to_string(),
        replacement: "false".to_string(),
        description: "adversarial test mutant".to_string(),
    }
}

fn source_root(tag: &str, contents: &[u8]) -> PathBuf {
    let root = fs::tempdir(tag);
    std::fs::write(root.join("fixture.rs"), contents).unwrap();
    root
}

#[cfg(unix)]
fn run_restore_case(root: &Path, command: &str) -> (PathBuf, MutantExecutionResult) {
    let outside = root.join("outside.txt");
    std::fs::write(&outside, b"outside\n").unwrap();
    let runner = NativeMutationRunner::new(1, Some(command.to_string()));
    let result = runner.run_mutant(&mutant("fixture.rs"), root);
    (outside, result)
}

#[cfg(unix)]
#[test]
fn passing_baseline_source_mutation_is_runner_error_and_restored_exactly() {
    use std::os::unix::fs::PermissionsExt;

    let root = source_root("mutation-runner-baseline-integrity", b"fn main() {}\n");
    let target = root.join("fixture.rs");
    let marker = root.join("baseline-ran");
    let mut permissions = std::fs::metadata(&target).unwrap().permissions();
    permissions.set_mode(0o640);
    std::fs::set_permissions(&target, permissions).unwrap();
    let original_mode = std::fs::metadata(&target).unwrap().permissions().mode();
    let runner = NativeMutationRunner::new(
        2,
        Some(format!(
            "sh -c 'printf changed > fixture.rs; chmod 600 fixture.rs; printf ran > {}'",
            marker.display()
        )),
    );

    let result = runner.run_baseline(Path::new("fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, BaselineOutcome::RunnerError);
    assert!(result.diagnostic.contains("modified source"));
    assert_eq!(std::fs::read(&target).unwrap(), b"fn main() {}\n");
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode(),
        original_mode
    );
    assert_eq!(std::fs::read(&marker).unwrap(), b"ran");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn restoration_refuses_symlink_sabotage_without_touching_external_file() {
    let root = source_root("mutation-runner-restore-symlink", b"true\n");
    let target = root.join("fixture.rs");
    let (outside, result) = run_restore_case(
        &root,
        "sh -c 'rm -f fixture.rs; ln -s outside.txt fixture.rs'",
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
    let runner = NativeMutationRunner::new(
        2,
        Some("sh -c 'rm -f fixture.rs; mkdir fixture.rs'".to_string()),
    );
    let result = runner.run_mutant(&mutant("fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(!result.source_restored);
    assert!(target.is_dir());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn restoration_refuses_symlinked_ancestor_without_touching_external_file() {
    let root = fs::tempdir("mutation-runner-restore-ancestor");
    let outside = fs::tempdir("mutation-runner-restore-ancestor-outside");
    let nested = root.join("nested");
    let target = nested.join("fixture.rs");
    let outside_target = outside.join("fixture.rs");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(&target, b"true\n").unwrap();
    std::fs::write(&outside_target, b"outside\n").unwrap();
    let command = format!("sh -c 'rm -rf nested; ln -s {} nested'", outside.display());
    let runner = NativeMutationRunner::new(2, Some(command));
    let result = runner.run_mutant(&mutant("nested/fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(!result.source_restored);
    assert_eq!(std::fs::read(&outside_target).unwrap(), b"outside\n");
    assert!(
        std::fs::symlink_metadata(&nested)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[cfg(unix)]
#[test]
fn direct_runner_rejects_absolute_mutant_outside_root() {
    let root = source_root("mutation-runner-absolute-outside", b"true\n");
    let outside = fs::tempdir("mutation-runner-absolute-outside-file");
    let outside_target = outside.join("fixture.rs");
    std::fs::write(&outside_target, b"outside\n").unwrap();
    let mut outside_mutant = mutant("fixture.rs");
    outside_mutant.file = outside_target.clone();
    let runner = NativeMutationRunner::new(2, Some("printf should-not-run".to_string()));

    let result = runner.run_mutant(&outside_mutant, Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(!result.source_restored);
    assert!(result.diagnostic.contains("outside repository root"));
    assert_eq!(std::fs::read(&outside_target).unwrap(), b"outside\n");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[cfg(unix)]
#[test]
fn atomic_replacement_leaves_outside_hardlink_peer_unchanged() {
    let root = source_root("mutation-runner-hardlink-outside", b"true\n");
    let outside = fs::tempdir("mutation-runner-hardlink-outside-peer");
    let target = root.join("fixture.rs");
    let peer = outside.join("peer.rs");
    std::fs::hard_link(&target, &peer).unwrap();
    let runner = NativeMutationRunner::new(2, Some("printf executed".to_string()));

    let result = runner.run_mutant(&mutant("fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::Survived);
    assert!(result.source_restored);
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    assert_eq!(std::fs::read(&peer).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[cfg(unix)]
#[test]
fn timeout_reserves_critical_diagnostic_after_saturated_output() {
    let root = source_root("mutation-runner-timeout-output", b"true\n");
    let runner = NativeMutationRunner::new(1, Some("yes timeout-output".to_string()));

    let result = runner.run_mutant(&mutant("fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::Timeout);
    assert!(result.source_restored);
    assert!(result.diagnostic.len() <= 64 * 1024);
    assert!(
        result
            .diagnostic
            .contains("Command timed out; process group terminated and absence was verified.")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn restoration_diagnostic_survives_saturated_output() {
    let root = source_root("mutation-runner-restore-output", b"true\n");
    let (outside, result) = run_restore_case(
        &root,
        "sh -c 'rm -f fixture.rs; ln -s outside.txt fixture.rs; yes output'",
    );

    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(!result.source_restored);
    assert!(result.diagnostic.len() <= 64 * 1024);
    assert!(result.diagnostic.contains("Failed to restore and verify"));
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn exited_descendants_with_inherited_pipes_are_cleaned_repeatedly() {
    let root = source_root("mutation-runner-inherited-pipes", b"fn main() {}\n");
    let runner = NativeMutationRunner::new(2, Some("sh -c 'sleep 30 & exit 0'".to_string()));
    let started = std::time::Instant::now();
    for _ in 0..3 {
        let result = runner.run_baseline(Path::new("fixture.rs"), Path::new(&root));
        assert_eq!(result.outcome, BaselineOutcome::Passed);
        assert!(result.diagnostic.len() <= 64 * 1024);
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(8),
        "inherited pipes exceeded bounded cleanup"
    );
    let _ = std::fs::remove_dir_all(root);
}
