#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::{MutationInput, apply_mutant_bytes, execute_applied};
use crate::engines::mutation::test_support::temp_root;
use crate::engines::mutation::{AstMutant, MutantOutcome, NativeMutationRunner};
use std::fs;
use std::path::{Path, PathBuf};

fn mutant() -> AstMutant {
    AstMutant {
        id: 1,
        file: PathBuf::from("fixture.rs"),
        line: 1,
        column: 1,
        start_byte: 0,
        end_byte: 4,
        original: "true".to_string(),
        replacement: "false".to_string(),
        description: "apply test mutant".to_string(),
    }
}

fn fixture(
    label: &str,
) -> (
    PathBuf,
    PathBuf,
    super::super::restore::RestoreLocation,
    super::super::restore::SourceSnapshot,
) {
    let root = temp_root("hardgate-apply", label);
    let target = root.join("fixture.rs");
    fs::write(&target, b"true\n").unwrap();
    let location = super::super::restore::open_location(&target, &root).unwrap();
    let original = super::super::restore::snapshot_location(&location)
        .unwrap()
        .unwrap();
    (root, target, location, original)
}

#[test]
fn execute_applied_rejects_unarmed_replacement_without_running_command() {
    let (root, target, location, original) = fixture("unarmed");
    let marker = root.join("executed");
    let command = format!("touch {}", marker.display());
    let plan = super::super::plan::custom_plan(&command, &target, &root);
    let runner = NativeMutationRunner::new(2, Some(command));
    let mutant = mutant();
    let mut expected = None;

    let execution = execute_applied(
        &runner,
        MutationInput {
            mutant: &mutant,
            location: &location,
            original: &original,
            plan: &plan,
        },
        &mut expected,
    );

    assert_eq!(execution.outcome, MutantOutcome::RunnerError);
    assert!(execution.diagnostic.contains("not armed"));
    assert!(!marker.exists());
    assert_eq!(fs::read(&target).unwrap(), b"true\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn execute_applied_rejects_mismatched_replacement_without_running_command() {
    let (root, target, location, original) = fixture("mismatched");
    let marker = root.join("executed");
    let command = format!("touch {}", marker.display());
    let plan = super::super::plan::custom_plan(&command, &target, &root);
    let runner = NativeMutationRunner::new(2, Some(command));
    let mutant = mutant();
    let mismatch = super::super::restore::SourceSnapshot {
        bytes: b"unexpected\n".to_vec(),
        permissions: original.permissions.clone(),
        identity: original.identity,
    };
    let mut expected = Some(mismatch);

    let execution = execute_applied(
        &runner,
        MutationInput {
            mutant: &mutant,
            location: &location,
            original: &original,
            plan: &plan,
        },
        &mut expected,
    );

    assert_eq!(execution.outcome, MutantOutcome::RunnerError);
    assert!(execution.diagnostic.contains("changed immediately"));
    assert!(!marker.exists());
    assert_eq!(fs::read(&target).unwrap(), b"true\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn execute_applied_reports_missing_and_nonregular_targets() {
    let (root, target, location, original) = fixture("missing-and-directory");
    let marker = root.join("executed");
    let command = format!("touch {}", marker.display());
    let plan = super::super::plan::custom_plan(&command, &target, &root);
    let runner = NativeMutationRunner::new(2, Some(command));
    let mutant = mutant();

    fs::remove_file(&target).unwrap();
    let mut missing_expected = None;
    let missing = execute_applied(
        &runner,
        MutationInput {
            mutant: &mutant,
            location: &location,
            original: &original,
            plan: &plan,
        },
        &mut missing_expected,
    );
    assert_eq!(missing.outcome, MutantOutcome::RunnerError);
    assert!(missing.diagnostic.contains("disappeared"));

    fs::create_dir(&target).unwrap();
    let mut directory_expected = None;
    let directory = execute_applied(
        &runner,
        MutationInput {
            mutant: &mutant,
            location: &location,
            original: &original,
            plan: &plan,
        },
        &mut directory_expected,
    );
    assert_eq!(directory.outcome, MutantOutcome::RunnerError);
    assert!(directory.diagnostic.contains("not a regular file"));
    assert!(!marker.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn apply_mutant_bytes_reports_disappeared_target_without_recreating_it() {
    let (root, target, location, original) = fixture("apply-missing");
    let mutant = mutant();
    fs::remove_file(&target).unwrap();
    let mut armed = None;

    let error = match apply_mutant_bytes(&location, &mutant, &original, &mut armed) {
        Ok(_) => panic!("missing target unexpectedly accepted"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(
        error
            .to_string()
            .contains("disappeared after its initial snapshot")
    );
    assert!(!target.exists());
    let _ = fs::remove_dir_all(root);
}
