use super::*;
use std::fs;

fn fixture_root(label: &str, file: &str, bytes: &[u8]) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("hardgate-runner-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let target = root.join(file);
    fs::write(&target, bytes).unwrap();
    (root, target)
}

fn fixture_mutant(file: &str, description: &str) -> AstMutant {
    AstMutant {
        id: 1,
        file: PathBuf::from(file),
        line: 1,
        column: 1,
        start_byte: 0,
        end_byte: 4,
        original: "true".to_string(),
        replacement: "false".to_string(),
        description: description.to_string(),
    }
}

fn prepared_fixture(
    label: &str,
    file: &str,
    description: &str,
) -> (PathBuf, PathBuf, AstMutant, PreparedTarget) {
    let (root, target) = fixture_root(label, file, b"true\n");
    let mutant = fixture_mutant(file, description);
    let prepared = prepare_target(&mutant, &root).unwrap();
    (root, target, mutant, prepared)
}

fn opened_fixture(label: &str) -> (PathBuf, PathBuf, RestoreLocation, SourceSnapshot) {
    let (root, target) = fixture_root(label, "fixture.rs", b"original\n");
    let location = super::restore::open_location(&target, &root).unwrap();
    let original = super::restore::snapshot_location(&location)
        .unwrap()
        .unwrap();
    (root, target, location, original)
}

#[derive(Clone, Copy)]
enum MismatchKind {
    Present,
    Missing,
}

fn assert_expected_mismatch(label: &str, kind: MismatchKind) {
    let (root, target, location, original) = opened_fixture(label);
    let observed = match kind {
        MismatchKind::Present => {
            fs::write(&target, b"baseline-change\n").unwrap();
            Some(
                super::restore::snapshot_location(&location)
                    .unwrap()
                    .unwrap(),
            )
        }
        MismatchKind::Missing => {
            fs::remove_file(&target).unwrap();
            None
        }
    };
    let (expected, concurrent, marker) = match kind {
        MismatchKind::Present => (
            observed
                .as_ref()
                .map_or(super::restore::ExpectedEntry::Missing, |snapshot| {
                    super::restore::ExpectedEntry::Present(snapshot)
                }),
            b"concurrent-replacement\n".as_slice(),
            "changed before descriptor-relative",
        ),
        MismatchKind::Missing => (
            super::restore::ExpectedEntry::Missing,
            b"concurrent-recreation\n".as_slice(),
            "recreated",
        ),
    };
    fs::write(&target, concurrent).unwrap();
    let error = super::restore::restore_location(&location, &original, expected).unwrap_err();

    assert!(error.to_string().contains(marker));
    assert_eq!(fs::read(&target).unwrap(), concurrent);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn preapply_external_edit_is_preserved_and_reported() {
    let (root, target, mutant, prepared) =
        prepared_fixture("preapply-edit", "fixture.rs", "pre-apply edit");
    let marker = root.join("executed");
    fs::write(&target, b"external\n").unwrap();
    let plan = plan::custom_plan(&format!("touch {}", marker.display()), &target, &root);
    let (execution, restored) = execute_and_restore(MutationContext {
        runner: &NativeMutationRunner::new(2, Some(plan.command.clone())),
        mutant: &mutant,
        target_path: &prepared.target_path,
        location: &prepared.location,
        original: &prepared.original,
        plan: &plan,
    });

    assert_eq!(execution.outcome, MutantOutcome::RunnerError);
    assert!(!restored);
    assert!(
        execution
            .diagnostic
            .contains("changed after its initial snapshot")
    );
    assert_eq!(fs::read(&target).unwrap(), b"external\n");
    assert!(!marker.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn no_write_timeout_guard_rejects_external_edit() {
    let (root, target, mutant, prepared) =
        prepared_fixture("no-write-edit", "fixture.js", "no-write guard");
    fs::write(&target, b"external\n").unwrap();
    let mut result = mutant_error(
        &mutant,
        "full-suite",
        Instant::now(),
        "full-suite timeout guard".to_string(),
    );

    verify_no_write(
        &mut result,
        &prepared.location,
        &prepared.original,
        &prepared.target_path,
    );

    assert!(!result.source_restored);
    assert!(
        result
            .diagnostic
            .contains("Failed to verify unchanged source")
    );
    assert_eq!(fs::read(&target).unwrap(), b"external\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolver_failure_after_target_snapshot_verifies_no_write() {
    let (root, target, _mutant, prepared) =
        prepared_fixture("resolver-no-write", "fixture.ts", "resolver failure");

    let error = resolution_failure_after_prepare(
        anyhow::anyhow!("malformed package metadata"),
        &prepared.location,
        &prepared.original,
        &prepared.target_path,
    );

    assert!(matches!(error, MutationRunnerError::Resolution(_)));
    assert_eq!(fs::read(&target).unwrap(), b"true\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolver_failure_never_overwrites_a_concurrent_source_edit() {
    let (root, target, _mutant, prepared) =
        prepared_fixture("resolver-integrity", "fixture.ts", "resolver failure");
    fs::write(&target, b"external\n").unwrap();

    let error = resolution_failure_after_prepare(
        anyhow::anyhow!("malformed package metadata"),
        &prepared.location,
        &prepared.original,
        &prepared.target_path,
    );

    assert!(matches!(error, MutationRunnerError::Integrity(_)));
    assert_eq!(fs::read(&target).unwrap(), b"external\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn expected_present_mismatch_preserves_concurrent_replacement() {
    assert_expected_mismatch("expected-present", MismatchKind::Present);
}

#[test]
fn expected_missing_mismatch_preserves_concurrent_recreation() {
    assert_expected_mismatch("expected-missing", MismatchKind::Missing);
}

#[test]
fn expected_missing_refuses_symlink_and_directory_targets() {
    use std::os::unix::fs::symlink;

    let (root, target, location, original) = opened_fixture("expected-missing-nonregular");
    let outside = root.join("outside.txt");
    fs::write(&outside, b"outside\n").unwrap();
    fs::remove_file(&target).unwrap();
    symlink(&outside, &target).unwrap();

    let symlink_error = super::restore::restore_location(
        &location,
        &original,
        super::restore::ExpectedEntry::Missing,
    )
    .unwrap_err();
    assert_eq!(symlink_error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(symlink_error.to_string().contains("target is a symlink"));
    assert_eq!(fs::read(&outside).unwrap(), b"outside\n");

    fs::remove_file(&target).unwrap();
    fs::create_dir(&target).unwrap();
    let directory_error = super::restore::restore_location(
        &location,
        &original,
        super::restore::ExpectedEntry::Missing,
    )
    .unwrap_err();
    assert_eq!(directory_error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        directory_error
            .to_string()
            .contains("target is not a regular file")
    );

    fs::remove_dir(&target).unwrap();
    let _ = fs::remove_dir_all(root);
}
