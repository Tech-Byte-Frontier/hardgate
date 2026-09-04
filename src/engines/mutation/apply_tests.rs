#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::{MutationInput, apply_mutant_bytes, execute_applied};
use crate::engines::mutation::ResolvedTestPlan;
use crate::engines::mutation::{AstMutant, MutantOutcome, NativeMutationRunner};
use std::fs;
use std::path::PathBuf;

use super::super::process::CommandExecution;
use super::super::restore::test_support;
use super::super::restore::{RestoreLocation, SourceSnapshot};

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

fn fixture(label: &str) -> (PathBuf, PathBuf, RestoreLocation, SourceSnapshot) {
    test_support::fixture("hardgate-apply", label, b"true\n")
}

struct ApplyHarness {
    root: PathBuf,
    target: PathBuf,
    location: RestoreLocation,
    original: SourceSnapshot,
    marker: PathBuf,
    runner: NativeMutationRunner,
    mutant: AstMutant,
    plan: ResolvedTestPlan,
}

fn harness(label: &str) -> ApplyHarness {
    let (root, target, location, original) = fixture(label);
    let marker = root.join("executed");
    let command = format!("touch {}", marker.display());
    ApplyHarness {
        plan: super::super::plan::custom_plan(&command, &target, &root),
        runner: NativeMutationRunner::new(2, Some(command)),
        mutant: mutant(),
        root,
        target,
        location,
        original,
        marker,
    }
}

fn execute(harness: &ApplyHarness, expected: &mut Option<SourceSnapshot>) -> CommandExecution {
    execute_applied(
        &harness.runner,
        MutationInput {
            mutant: &harness.mutant,
            location: &harness.location,
            original: &harness.original,
            plan: &harness.plan,
        },
        expected,
    )
}

fn assert_rejected(harness: &ApplyHarness, execution: &CommandExecution, message: &str) {
    assert_eq!(execution.outcome, MutantOutcome::RunnerError);
    assert!(execution.diagnostic.contains(message));
    assert!(!harness.marker.exists());
    assert_eq!(fs::read(&harness.target).unwrap(), b"true\n");
}

#[test]
fn execute_applied_rejects_unarmed_replacement_without_running_command() {
    let harness = harness("unarmed");
    let mut expected = None;

    let execution = execute(&harness, &mut expected);

    assert_rejected(&harness, &execution, "not armed");
    let _ = fs::remove_dir_all(harness.root);
}

#[test]
fn execute_applied_rejects_mismatched_replacement_without_running_command() {
    let harness = harness("mismatched");
    let mismatch = SourceSnapshot {
        bytes: b"unexpected\n".to_vec(),
        permissions: harness.original.permissions.clone(),
        identity: harness.original.identity,
    };
    let mut expected = Some(mismatch);

    let execution = execute(&harness, &mut expected);

    assert_rejected(&harness, &execution, "changed immediately");
    let _ = fs::remove_dir_all(harness.root);
}

#[test]
fn execute_applied_reports_missing_and_nonregular_targets() {
    let harness = harness("missing-and-directory");

    fs::remove_file(&harness.target).unwrap();
    let mut missing_expected = None;
    let missing = execute(&harness, &mut missing_expected);
    assert_eq!(missing.outcome, MutantOutcome::RunnerError);
    assert!(missing.diagnostic.contains("disappeared"));

    fs::create_dir(&harness.target).unwrap();
    let mut directory_expected = None;
    let directory = execute(&harness, &mut directory_expected);
    assert_eq!(directory.outcome, MutantOutcome::RunnerError);
    assert!(directory.diagnostic.contains("not a regular file"));
    assert!(!harness.marker.exists());
    let _ = fs::remove_dir_all(harness.root);
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

    test_support::assert_error(
        &error,
        std::io::ErrorKind::NotFound,
        "disappeared after its initial snapshot",
    );
    assert!(!target.exists());
    let _ = fs::remove_dir_all(root);
}
