use super::super::mutation_output::{
    MutationFailure, MutationNoop, MutationSummaryContext, baseline_failure, render_mutation_noop,
    render_mutation_output, runtime_failure,
};
use super::{
    increment_stats, mutation_run_passed, outcome_label, print_outcome, round_robin_mutants,
    run_mutant_batch, take_next_family,
};
use crate::engines::mutation::runner::{BaselineSources, MutationRunnerError};
use crate::engines::mutation::test_support::temp_root;
use crate::engines::{
    AstMutant, BaselineExecutionResult, BaselineOutcome, MutantExecutionResult, MutantOutcome,
    MutationStats, NativeMutationRunner,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn mutant(id: usize) -> AstMutant {
    AstMutant {
        id,
        file: PathBuf::from("nested/fixture.rs"),
        line: 1,
        column: 1,
        start_byte: 0,
        end_byte: 4,
        original: "true".to_string(),
        replacement: "false".to_string(),
        description: "batch safety mutant".to_string(),
    }
}

fn snapshot_source(root: &Path, file: &str) -> BaselineSources {
    NativeMutationRunner::snapshot_baseline_sources(&[PathBuf::from(file)], root).unwrap()
}

fn assert_runner_error(result: BaselineExecutionResult, marker: &Path) {
    assert_eq!(result.outcome, crate::engines::BaselineOutcome::RunnerError);
    assert!(!marker.exists());
}

fn execution(outcome: MutantOutcome, diagnostic: &str) -> MutantExecutionResult {
    MutantExecutionResult {
        mutant: mutant(1),
        outcome,
        duration_ms: 0,
        command: "true".to_string(),
        diagnostic: diagnostic.to_string(),
        source_restored: true,
    }
}

fn baseline(outcome: BaselineOutcome, diagnostic: &str) -> BaselineExecutionResult {
    BaselineExecutionResult {
        file: PathBuf::from("target.rs"),
        outcome,
        duration_ms: 0,
        command: "true".to_string(),
        diagnostic: diagnostic.to_string(),
    }
}

fn stats(
    [
        killed,
        survived,
        timeout,
        compile_error,
        runner_error,
        unviable,
    ]: [usize; 6],
) -> MutationStats {
    MutationStats {
        killed,
        survived,
        timeout,
        compile_error,
        runner_error,
        equivalent: 0,
        unviable,
        total: killed + survived + timeout + compile_error + runner_error + unviable,
    }
}

#[test]
fn baseline_snapshot_failure_is_typed_before_commands() {
    let root = temp_root("hardgate", "baseline-snapshot-failure");
    let protected = [PathBuf::from("missing.rs")];
    let runner = NativeMutationRunner::new(1, Some("true".to_string()));
    let result = super::run_unmutated_baselines(super::BaselineRun {
        runner: &runner,
        command_files: &[],
        protected_files: &protected,
        root: Path::new(&root),
        json: true,
    })
    .expect_err("missing protected source must fail before baseline commands");
    let failure = result
        .downcast_ref::<MutationFailure>()
        .expect("snapshot failure should preserve MutationFailure");
    assert_eq!(failure.stage, "baseline");
    assert_eq!(failure.kind, "source-integrity-error");
    assert!(failure.message.contains("snapshot protected"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mutation_outcome_helpers_cover_all_variants() {
    for outcome in [
        MutantOutcome::Killed,
        MutantOutcome::Survived,
        MutantOutcome::Timeout,
        MutantOutcome::CompileError,
        MutantOutcome::RunnerError,
        MutantOutcome::Equivalent,
        MutantOutcome::Unviable,
    ] {
        let _ = outcome_label(outcome);
    }
    let mut counted = MutationStats::default();
    for outcome in [
        MutantOutcome::Killed,
        MutantOutcome::Survived,
        MutantOutcome::Timeout,
        MutantOutcome::CompileError,
        MutantOutcome::RunnerError,
        MutantOutcome::Equivalent,
        MutantOutcome::Unviable,
    ] {
        increment_stats(&mut counted, outcome);
    }
    assert_eq!(counted.killed, 1);
    assert_eq!(counted.unviable, 1);

    let mut styled = MutationStats::default();
    for outcome in [
        MutantOutcome::Killed,
        MutantOutcome::Survived,
        MutantOutcome::Timeout,
        MutantOutcome::CompileError,
        MutantOutcome::RunnerError,
        MutantOutcome::Equivalent,
        MutantOutcome::Unviable,
    ] {
        print_outcome(&mut styled, outcome);
    }
    assert_eq!(styled.killed, 1);
    assert_eq!(styled.unviable, 1);
}

#[test]
fn mutation_failure_mappings_preserve_stages_and_diagnostics() {
    let integrity = MutationFailure::from_runner_error(MutationRunnerError::Integrity(
        "source changed".to_string(),
    ));
    assert_eq!(integrity.stage, "execution");
    assert_eq!(integrity.kind, "execution-error");
    let resolution = MutationFailure::from_runner_error(MutationRunnerError::Resolution(
        "test plan unavailable".to_string(),
    ));
    assert_eq!(resolution.stage, "resolution");
    assert_eq!(resolution.kind, "resolution-error");

    let empty_diagnostic = baseline(BaselineOutcome::Failed, "");
    assert!(
        baseline_failure(&empty_diagnostic, Path::new("target.rs"))
            .to_string()
            .contains("no diagnostic output")
    );
    for outcome in [
        BaselineOutcome::Timeout,
        BaselineOutcome::RunnerError,
        BaselineOutcome::Passed,
    ] {
        assert!(
            baseline_failure(&baseline(outcome, "diagnostic"), Path::new("target.rs"))
                .to_string()
                .contains("unmutated baseline")
        );
    }

    for outcome in [MutantOutcome::RunnerError, MutantOutcome::Timeout] {
        let failure = runtime_failure(&execution(outcome, "diagnostic"))
            .expect("runtime failures should be typed")
            .to_string();
        assert!(failure.contains("mutant"));
    }
    assert!(runtime_failure(&execution(MutantOutcome::Killed, "")).is_none());
    assert!(
        runtime_failure(&execution(MutantOutcome::RunnerError, ""))
            .expect("empty diagnostics still report runtime failures")
            .to_string()
            .contains("no diagnostic output")
    );
}

#[test]
fn mutation_output_modes_and_noops_are_rendered() {
    let result = execution(MutantOutcome::Survived, "");
    let stats = stats([0, 1, 0, 0, 0, 0]);
    let context = MutationSummaryContext {
        stats: &stats,
        results: std::slice::from_ref(&result),
        score: 0.0,
        min_score: 0.0,
        passed: true,
        elapsed: 1,
    };
    render_mutation_output(&context, Some("agent")).unwrap();
    render_mutation_output(&context, Some("json")).unwrap();
    render_mutation_output(&context, None).unwrap();
    for format in [Some("json"), None] {
        render_mutation_noop(
            MutationNoop {
                passed: true,
                status: "noop",
                stage: "selection",
                kind: "empty",
                message: "nothing selected",
            },
            format,
        )
        .unwrap();
    }
}

#[test]
fn mutation_selection_and_verdict_guards_cover_empty_and_failure_paths() {
    let mut empty_families = BTreeMap::<(String, String), Vec<AstMutant>>::new();
    assert!(take_next_family(&mut empty_families, &mut 0).is_none());
    let mut exhausted = BTreeMap::from([(("true".to_string(), "false".to_string()), vec![])]);
    assert!(take_next_family(&mut exhausted, &mut 0).is_none());

    let grouped = BTreeMap::from([(
        PathBuf::from("nested/fixture.rs"),
        BTreeMap::from([(("true".to_string(), "false".to_string()), vec![mutant(1)])]),
    )]);
    assert_eq!(round_robin_mutants(grouped, 1).len(), 1);
    let exhausted_group = BTreeMap::from([(
        PathBuf::from("nested/fixture.rs"),
        BTreeMap::from([(("true".to_string(), "false".to_string()), vec![])]),
    )]);
    assert!(round_robin_mutants(exhausted_group, 1).is_empty());

    assert!(!mutation_run_passed(&stats([0, 0, 0, 0, 0, 0]), 0.0, 0.0));
    assert!(!mutation_run_passed(&stats([1, 0, 0, 0, 0, 0]), 0.0, 1.0));
    assert!(!mutation_run_passed(&stats([1, 0, 1, 0, 0, 0]), 100.0, 0.0));
    assert!(!mutation_run_passed(&stats([1, 0, 0, 1, 0, 0]), 100.0, 0.0));
    assert!(!mutation_run_passed(&stats([1, 0, 0, 0, 1, 0]), 100.0, 0.0));
    assert!(!mutation_run_passed(&stats([1, 0, 0, 0, 0, 1]), 100.0, 0.0));
    assert!(mutation_run_passed(&stats([1, 0, 0, 0, 0, 0]), 100.0, 85.0));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baselines_protect_every_production_target_before_mutants() {
    let root = temp_root("hardgate", "baseline-set");
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    std::fs::write(&first, b"true\n").unwrap();
    std::fs::write(&second, b"false\n").unwrap();
    let runner = NativeMutationRunner::new(
        2,
        Some("sh -c 'printf altered > second.rs; chmod 600 second.rs'".to_string()),
    );
    let files = [PathBuf::from("first.rs"), PathBuf::from("second.rs")];

    let result = super::run_unmutated_baselines(super::BaselineRun {
        runner: &runner,
        command_files: &files,
        protected_files: &files,
        root: Path::new(&root),
        json: false,
    });

    assert!(result.is_err());
    assert_eq!(std::fs::read(&first).unwrap(), b"true\n");
    assert_eq!(std::fs::read(&second).unwrap(), b"false\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_rejects_preexisting_hardlink_before_running_command() {
    let root = temp_root("hardgate", "baseline-hardlink");
    let target = root.join("target.rs");
    let peer = root.join("peer.rs");
    let marker = root.join("ran");
    std::fs::write(&target, b"true\n").unwrap();
    std::fs::hard_link(&target, &peer).unwrap();
    let runner = NativeMutationRunner::new(2, Some(format!("sh -c 'touch {}'", marker.display())));

    let result = runner.run_baseline(Path::new("target.rs"), Path::new(&root));

    assert_runner_error(result, &marker);
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    assert_eq!(std::fs::read(&peer).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_preflight_restores_external_change_before_running_command() {
    let root = temp_root("hardgate", "baseline-preflight");
    let target = root.join("target.rs");
    let marker = root.join("ran");
    std::fs::write(&target, b"true\n").unwrap();
    let runner = NativeMutationRunner::new(2, Some(format!("sh -c 'touch {}'", marker.display())));
    let protected = snapshot_source(&root, "target.rs");
    std::fs::write(&target, b"changed\n").unwrap();

    let result =
        runner.run_baseline_with_sources(Path::new("target.rs"), Path::new(&root), &protected);

    assert_runner_error(result, &marker);
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_resolution_failure_preserves_a_concurrent_source_edit() {
    let root = temp_root("hardgate", "baseline-resolution-integrity");
    let target = root.join("target.ts");
    std::fs::write(&target, b"export const value = true;\n").unwrap();
    std::fs::write(root.join("package.json"), b"{\n").unwrap();
    let runner = NativeMutationRunner::new(2, None);
    let protected = snapshot_source(&root, "target.ts");
    std::fs::write(&target, b"external\n").unwrap();

    let error = runner
        .resolve_baseline_plan(Path::new("target.ts"), Path::new(&root), &protected)
        .unwrap_err();

    assert!(matches!(error, MutationRunnerError::Integrity(_)));
    assert_eq!(std::fs::read(&target).unwrap(), b"external\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_restores_same_bytes_replacement_and_detaches_hardlink() {
    let root = temp_root("hardgate", "baseline-replacement");
    let outside = temp_root("hardgate", "baseline-replacement-peer");
    let target = root.join("target.rs");
    let peer = outside.join("peer.rs");
    std::fs::write(&target, b"true\n").unwrap();
    std::fs::write(&peer, b"true\n").unwrap();
    let runner = NativeMutationRunner::new(
        2,
        Some(format!(
            "sh -c 'rm target.rs; ln {} target.rs'",
            peer.display()
        )),
    );

    let result = runner.run_baseline(Path::new("target.rs"), Path::new(&root));

    assert_eq!(result.outcome, crate::engines::BaselineOutcome::RunnerError);
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    assert_eq!(std::fs::read(&peer).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn batch_aborts_after_restore_failure_before_starting_later_mutants() {
    let root = temp_root("hardgate", "batch-restore");
    let outside = temp_root("hardgate", "batch-restore-outside");
    let nested = root.join("nested");
    let outside_target = outside.join("fixture.rs");
    let second_marker = root.join("second-mutant-started");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("fixture.rs"), b"true\n").unwrap();
    std::fs::write(&outside_target, b"outside\n").unwrap();
    let command = format!(
        "sh -c 'if [ ! -e batch-first ]; then touch batch-first; rm -rf nested; ln -s {} nested; else touch {}; fi'",
        outside.display(),
        second_marker.display()
    );
    let runner = NativeMutationRunner::new(2, Some(command));
    let mutants = [mutant(1), mutant(2)];

    let result = run_mutant_batch(&mutants, &runner, Path::new(&root), false);

    assert!(result.is_err());
    assert!(!second_marker.exists());
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
