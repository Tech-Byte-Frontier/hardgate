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

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn detached_parent_fixture(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = fs::tempdir(tag);
    let nested = root.join("nested");
    let detached = root.join("nested.detached");
    let live_target = nested.join("fixture.rs");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(&live_target, b"true\n").unwrap();
    (root, nested, detached, live_target)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_restore_case(root: &Path, command: &str) -> (PathBuf, MutantExecutionResult) {
    let outside = root.join("outside.txt");
    std::fs::write(&outside, b"outside\n").unwrap();
    let runner = NativeMutationRunner::new(1, Some(command.to_string()));
    let result = runner.run_mutant(&mutant("fixture.rs"), root);
    (outside, result)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn rollback_refuses_post_apply_hardlink_replacement() {
    let root = source_root("mutation-runner-rollback-hardlink", b"true\n");
    let outside = fs::tempdir("mutation-runner-rollback-hardlink-peer");
    let outside_target = outside.join("fixture.rs");
    std::fs::write(&outside_target, b"outside\n").unwrap();
    let command = format!(
        "sh -c 'rm -f fixture.rs; ln {} fixture.rs'",
        outside_target.display()
    );
    let runner = NativeMutationRunner::new(2, Some(command));

    let result = runner.run_mutant(&mutant("fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(!result.source_restored);
    assert_eq!(std::fs::read(&outside_target).unwrap(), b"outside\n");
    assert_eq!(
        std::fs::read(root.join("fixture.rs")).unwrap(),
        b"outside\n"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_refuses_detached_parent_without_restoring_live_replacement() {
    let (root, _nested, detached, live_target) =
        detached_parent_fixture("mutation-runner-baseline-detached-parent");
    let runner = NativeMutationRunner::new(
        2,
        Some("sh -c 'mv nested nested.detached; mkdir nested; printf outside > nested/fixture.rs; printf changed > nested.detached/fixture.rs'".to_string()),
    );

    let result = runner.run_baseline(Path::new("nested/fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, BaselineOutcome::RunnerError);
    assert!(result.diagnostic.contains("parent identity changed"));
    assert_eq!(std::fs::read(&live_target).unwrap(), b"outside");
    assert_eq!(
        std::fs::read(detached.join("fixture.rs")).unwrap(),
        b"changed"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn rollback_refuses_detached_parent_and_does_not_write_old_directory() {
    let (root, _nested, detached, live_target) =
        detached_parent_fixture("mutation-runner-detached-parent");
    let command =
        "sh -c 'mv nested nested.detached; mkdir nested; printf outside > nested/fixture.rs'";
    let runner = NativeMutationRunner::new(2, Some(command.to_string()));

    let result = runner.run_mutant(&mutant("nested/fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(!result.source_restored);
    assert_eq!(std::fs::read(&live_target).unwrap(), b"outside");
    assert_eq!(
        std::fs::read(detached.join("fixture.rs")).unwrap(),
        b"false\n"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn direct_runner_rejects_symlink_parent_component_before_normalization() {
    use std::os::unix::fs::symlink;

    let root = source_root("mutation-runner-symlink-parent-dotdot", b"true\n");
    let outside_parent = fs::tempdir("mutation-runner-symlink-parent-dotdot-outside");
    let outside_target = outside_parent.join("fixture.rs");
    std::fs::write(&outside_target, b"outside\n").unwrap();
    symlink(&outside_parent, root.join("link")).unwrap();
    let marker = root.join("executed");
    let runner = NativeMutationRunner::new(2, Some(format!("touch {}", marker.display())));
    let escaped = mutant("link/../fixture.rs");

    let result = runner.run_mutant(&escaped, Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(!result.source_restored);
    assert!(result.diagnostic.contains("parent-directory components"));
    assert!(!marker.exists());
    assert_eq!(std::fs::read(&outside_target).unwrap(), b"outside\n");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside_parent);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn symlinked_repository_root_spelling_stays_contained() {
    use std::os::unix::fs::symlink;

    let actual = source_root("mutation-runner-root-spelling", b"true\n");
    let alias = fs::tempdir("mutation-runner-root-spelling-alias");
    std::fs::remove_dir_all(&alias).unwrap();
    symlink(&actual, &alias).unwrap();
    let runner = NativeMutationRunner::new(2, Some("printf executed".to_string()));

    let result = runner.run_mutant(&mutant("fixture.rs"), Path::new(&alias));

    assert_eq!(result.outcome, MutantOutcome::Survived);
    assert!(result.source_restored);
    assert_eq!(std::fs::read(actual.join("fixture.rs")).unwrap(), b"true\n");
    let _ = std::fs::remove_file(alias);
    let _ = std::fs::remove_dir_all(actual);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn preexisting_outside_hardlink_is_rejected_without_touching_peer() {
    let root = source_root("mutation-runner-hardlink-outside", b"true\n");
    let outside = fs::tempdir("mutation-runner-hardlink-outside-peer");
    let target = root.join("fixture.rs");
    let peer = outside.join("peer.rs");
    let marker = root.join("ran");
    std::fs::hard_link(&target, &peer).unwrap();
    let runner = NativeMutationRunner::new(
        2,
        Some(format!("sh -c 'printf executed > {}'", marker.display())),
    );

    let result = runner.run_mutant(&mutant("fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, MutantOutcome::RunnerError);
    assert!(result.source_restored);
    assert!(!marker.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    assert_eq!(std::fs::read(&peer).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn exited_descendant_with_closed_pipes_is_still_reaped() {
    let root = source_root("mutation-runner-closed-pipes", b"fn main() {}\n");
    let pid_file = root.join("child.pid");
    let command = format!(
        "sh -c 'sleep 30 >/dev/null 2>&1 & printf %s $! > {}; exit 0'",
        pid_file.display()
    );
    let runner = NativeMutationRunner::new(2, Some(command));

    let result = runner.run_baseline(Path::new("fixture.rs"), Path::new(&root));

    assert_eq!(result.outcome, BaselineOutcome::Passed);
    let child_pid = std::fs::read_to_string(&pid_file).unwrap();
    let child_pid = child_pid.trim().parse::<i32>().unwrap();
    let pid = rustix::process::Pid::from_raw(child_pid).unwrap();
    for _ in 0..100 {
        match rustix::io::retry_on_intr(|| rustix::process::test_kill_process(pid)) {
            Err(error) if error == rustix::io::Errno::SRCH => break,
            Ok(()) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(error) => panic!("closed-pipe descendant probe failed: {error}"),
        }
    }
    let still_alive = rustix::process::test_kill_process(pid).is_ok();
    assert!(!still_alive, "closed-pipe descendant survived cleanup");
    let _ = std::fs::remove_dir_all(root);
}
