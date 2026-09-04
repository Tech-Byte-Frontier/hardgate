use super::run_mutant_batch;
use crate::engines::mutation::runner::MutationRunnerError;
use crate::engines::{AstMutant, NativeMutationRunner};
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baselines_protect_every_production_target_before_mutants() {
    let root = std::env::temp_dir().join(format!("hardgate-baseline-set-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    std::fs::write(&first, b"true\n").unwrap();
    std::fs::write(&second, b"false\n").unwrap();
    let runner = NativeMutationRunner::new(
        2,
        Some("sh -c 'printf altered > second.rs; chmod 600 second.rs'".to_string()),
    );
    let files = [PathBuf::from("first.rs"), PathBuf::from("second.rs")];

    let result = super::run_unmutated_baselines(&runner, &files, &files, Path::new(&root), false);

    assert!(result.is_err());
    assert_eq!(std::fs::read(&first).unwrap(), b"true\n");
    assert_eq!(std::fs::read(&second).unwrap(), b"false\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_rejects_preexisting_hardlink_before_running_command() {
    let root =
        std::env::temp_dir().join(format!("hardgate-baseline-hardlink-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("target.rs");
    let peer = root.join("peer.rs");
    let marker = root.join("ran");
    std::fs::write(&target, b"true\n").unwrap();
    std::fs::hard_link(&target, &peer).unwrap();
    let runner = NativeMutationRunner::new(2, Some(format!("sh -c 'touch {}'", marker.display())));

    let result = runner.run_baseline(Path::new("target.rs"), Path::new(&root));

    assert_eq!(result.outcome, crate::engines::BaselineOutcome::RunnerError);
    assert!(!marker.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    assert_eq!(std::fs::read(&peer).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_preflight_restores_external_change_before_running_command() {
    let root = std::env::temp_dir().join(format!(
        "hardgate-baseline-preflight-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("target.rs");
    let marker = root.join("ran");
    std::fs::write(&target, b"true\n").unwrap();
    let runner = NativeMutationRunner::new(2, Some(format!("sh -c 'touch {}'", marker.display())));
    let protected = NativeMutationRunner::snapshot_baseline_sources(
        &[PathBuf::from("target.rs")],
        Path::new(&root),
    )
    .unwrap();
    std::fs::write(&target, b"changed\n").unwrap();

    let result =
        runner.run_baseline_with_sources(Path::new("target.rs"), Path::new(&root), &protected);

    assert_eq!(result.outcome, crate::engines::BaselineOutcome::RunnerError);
    assert!(!marker.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn baseline_resolution_failure_preserves_a_concurrent_source_edit() {
    let root = std::env::temp_dir().join(format!(
        "hardgate-baseline-resolution-integrity-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("target.ts");
    std::fs::write(&target, b"export const value = true;\n").unwrap();
    std::fs::write(root.join("package.json"), b"{\n").unwrap();
    let runner = NativeMutationRunner::new(2, None);
    let protected = NativeMutationRunner::snapshot_baseline_sources(
        &[PathBuf::from("target.ts")],
        Path::new(&root),
    )
    .unwrap();
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
    let root = std::env::temp_dir().join(format!(
        "hardgate-baseline-replacement-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "hardgate-baseline-replacement-peer-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
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
    let root = std::env::temp_dir().join(format!("hardgate-batch-restore-{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!(
        "hardgate-batch-restore-outside-{}",
        std::process::id()
    ));
    let nested = root.join("nested");
    let outside_target = outside.join("fixture.rs");
    let second_marker = root.join("second-mutant-started");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
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
