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
    let root = fs::tempdir("mutation-runner-compile");
    let target = root.join("fixture.rs");
    std::fs::write(&target, b"true\n").unwrap();
    let runner = NativeMutationRunner::new(
        2,
        Some("sh -c 'printf \"error[E0308]: mismatched types\\n\" >&2; exit 1'".to_string()),
    );
    let result = runner.run_mutant(&mutant("fixture.rs", 0, 4, "true"), Path::new(&root));
    assert_eq!(result.outcome, MutantOutcome::CompileError);
    assert!(result.source_restored);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn timeout_terminates_process_group_descendants() {
    let root = fs::tempdir("mutation-runner-timeout");
    let target = root.join("fixture.rs");
    let script = root.join("hang.sh");
    let pid_file = root.join("child.pid");
    std::fs::write(&target, b"true\n").unwrap();
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait \"$!\"\n",
            pid_file.display()
        ),
    )
    .unwrap();

    let runner = NativeMutationRunner::new(1, Some("sh hang.sh".to_string()));
    let result = runner.run_mutant(&mutant("fixture.rs", 0, 4, "true"), Path::new(&root));
    assert_eq!(result.outcome, MutantOutcome::Timeout);
    assert!(result.source_restored);

    let child_pid = std::fs::read_to_string(&pid_file).unwrap();
    let child_pid = child_pid.trim().parse::<i32>().unwrap();
    for _ in 0..100 {
        let alive = std::process::Command::new("kill")
            .args(["-0", &child_pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !alive {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(root);
    panic!("timeout left descendant process {child_pid} alive");
}
