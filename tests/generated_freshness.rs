use hardgate::config::GeneratedConfig;
use hardgate::engines::{OrchestrationResult, OrchestrationViolation, run_generated_freshness};
use std::path::{Path, PathBuf};

#[test]
fn disabled_freshness_executes_nothing() {
    let root = tempdir("generated-disabled");
    let marker = root.join("ran");
    let config = generated_config(false, Some("touch ran"), Some(1));

    assert!(run_generated_freshness(&config, &root).is_none());
    assert!(!marker.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn successful_freshness_returns_evidence() {
    let root = tempdir("generated-success");
    let config = generated_config(true, Some("printf freshness-evidence"), Some(1));

    let result = successful_result(&config, &root);
    assert_eq!(result.step, "generated-freshness");
    assert_eq!(result.output, "freshness-evidence");
    assert!(result.success);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn nonzero_freshness_is_a_violation() {
    let root = tempdir("generated-nonzero");
    let config = generated_config(
        true,
        Some("sh -c 'printf freshness-failure >&2; exit 7'"),
        Some(1),
    );

    assert_command_failure(
        &config,
        &root,
        FailureExpectation {
            exit_code: Some(7),
            output: "freshness-failure",
            recommendation: "generated artifacts",
        },
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn runner_failure_is_a_violation() {
    let root = tempdir("generated-runner-failure");
    let config = generated_config(
        true,
        Some("hardgate-generated-command-that-does-not-exist"),
        Some(1),
    );

    assert_command_failure(
        &config,
        &root,
        FailureExpectation {
            exit_code: None,
            output: "Failed to execute",
            recommendation: "installed",
        },
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn enabled_freshness_without_command_is_explicit_failure() {
    let root = tempdir("generated-missing");
    let config = generated_config(true, None, Some(1));

    let violation = run_generated_freshness(&config, &root)
        .expect("enabled malformed config should return a result")
        .expect_err("missing command must fail closed");
    assert_eq!(violation.step, "generated-freshness");
    assert!(violation.output.contains("no freshness_command"));
    assert!(
        violation
            .recommendation
            .contains("generated.freshness_command")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn freshness_output_is_bounded() {
    let root = tempdir("generated-output");
    let config = generated_config(true, Some("sh -c 'yes output | head -c 100000'"), Some(1));

    let result = successful_result(&config, &root);
    assert!(
        result.output.len() <= 64 * 1024,
        "{} bytes",
        result.output.len()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn freshness_timeout_terminates_descendants() {
    let root = tempdir("generated-timeout");
    let script = root.join("hang.sh");
    let pid_file = root.join("child.pid");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait\n",
            pid_file.display()
        ),
    )
    .unwrap();
    let config = generated_config(true, Some("sh hang.sh"), Some(1));

    let violation = run_generated_freshness(&config, &root)
        .expect("enabled freshness should execute")
        .expect_err("hung command must time out");
    assert!(violation.output.contains("timed out"), "{violation:?}");
    assert!(violation.output.contains("process group"), "{violation:?}");
    assert!(
        violation.recommendation.contains("generated.timeout_secs"),
        "{violation:?}"
    );

    let child_pid = std::fs::read_to_string(&pid_file).unwrap();
    let child_pid = child_pid.trim().parse::<i32>().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let alive = std::process::Command::new("kill")
        .args(["-0", &child_pid.to_string()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    let _ = std::fs::remove_dir_all(root);
    assert!(!alive, "timeout left descendant process {child_pid} alive");
}

fn generated_config(
    enabled: bool,
    freshness_command: Option<&str>,
    timeout_secs: Option<u64>,
) -> GeneratedConfig {
    GeneratedConfig {
        enabled,
        freshness_command: freshness_command.map(str::to_string),
        timeout_secs,
    }
}

fn successful_result(config: &GeneratedConfig, root: &Path) -> OrchestrationResult {
    run_generated_freshness(config, root)
        .expect("enabled freshness should execute")
        .expect("successful command should return evidence")
}

fn violation_result(config: &GeneratedConfig, root: &Path) -> OrchestrationViolation {
    run_generated_freshness(config, root)
        .expect("enabled freshness should execute")
        .expect_err("freshness command must fail closed")
}

fn assert_command_failure(config: &GeneratedConfig, root: &Path, expected: FailureExpectation<'_>) {
    let violation = violation_result(config, root);
    assert_eq!(violation.step, "generated-freshness");
    assert_eq!(violation.exit_code, expected.exit_code);
    assert!(violation.output.contains(expected.output));
    assert!(violation.recommendation.contains(expected.recommendation));
}

struct FailureExpectation<'a> {
    exit_code: Option<i32>,
    output: &'a str,
    recommendation: &'a str,
}

fn tempdir(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("hardgate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}
