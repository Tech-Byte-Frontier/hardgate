use hardgate::config::OrchestrationConfig;
use hardgate::engines::OrchestrationEngine;
use std::path::PathBuf;

fn engine(step: Option<&str>, timeout_secs: Option<u64>) -> OrchestrationEngine {
    OrchestrationEngine::new(&OrchestrationConfig {
        format_check: step.map(str::to_string),
        format: None,
        lint: None,
        test_cmd: None,
        timeout_secs,
        ..Default::default()
    })
}

#[test]
fn missing_command_is_an_actionable_runner_error() {
    let root = tempdir("missing_command_is_an_actionable_runner_error");
    let result = engine(Some("hardgate-command-that-does-not-exist"), Some(1))
        .run_format_check(&root)
        .expect("configured formatter should run")
        .expect_err("missing command must fail closed");

    assert!(result.output.contains("Failed to execute"), "{result:?}");
    assert!(result.recommendation.contains("installed"), "{result:?}");
    assert_eq!(result.exit_code, None);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn nonzero_command_preserves_exit_status_and_output() {
    let root = tempdir("nonzero_command_preserves_exit_status_and_output");
    let result = engine(
        Some("sh -c 'printf orchestration-failure >&2; exit 7'"),
        Some(1),
    )
    .run_format_check(&root)
    .expect("configured formatter should run")
    .expect_err("nonzero command must fail closed");

    assert_eq!(result.exit_code, Some(7));
    assert!(result.output.contains("orchestration-failure"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn command_output_is_bounded() {
    let root = tempdir("command_output_is_bounded");
    let result = engine(Some("sh -c 'yes output | head -c 200000'"), Some(1))
        .run_format_check(&root)
        .expect("configured formatter should run")
        .expect("bounded noisy command should still succeed");

    assert!(
        result.output.len() <= 64 * 1024,
        "{} bytes",
        result.output.len()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn timeout_terminates_descendant_processes() {
    let root = tempdir("orchestration-timeout");
    let script = root.join("hang.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nsleep 30 &\nprintf 'child-pid=%s\\n' \"$!\"\nwait\n",
    )
    .unwrap();

    let result = engine(Some("sh hang.sh"), Some(1))
        .run_format_check(&root)
        .expect("configured formatter should run")
        .expect_err("hung command must time out");
    assert!(result.output.contains("timed out"), "{result:?}");
    assert!(result.output.contains("process group"), "{result:?}");
    assert!(
        result
            .output
            .contains("terminated and absence was verified"),
        "{result:?}"
    );

    let child_pid = result
        .output
        .lines()
        .find_map(|line| line.strip_prefix("child-pid="))
        .unwrap();
    let child_pid = child_pid.trim().parse::<i32>().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let child_pid = rustix::process::Pid::from_raw(child_pid).unwrap();
    let alive = match rustix::io::retry_on_intr(|| rustix::process::test_kill_process(child_pid)) {
        Ok(()) => true,
        Err(error) if error == rustix::io::Errno::SRCH => false,
        Err(error) => panic!("descendant process probe failed: {error}"),
    };
    let _ = std::fs::remove_dir_all(root);
    assert!(!alive, "timeout left descendant process {child_pid} alive");
}

#[test]
fn check_all_runs_the_configured_test_command() {
    let root = tempdir("check_all_runs_the_configured_test_command");
    let config = OrchestrationConfig {
        format_check: Some("printf format".to_string()),
        format: None,
        lint: Some("printf lint".to_string()),
        test_cmd: Some("printf test".to_string()),
        timeout_secs: Some(1),
        ..Default::default()
    };
    let (results, violations) = OrchestrationEngine::new(&config).run_all_checks(&root);
    assert!(violations.is_empty(), "{violations:?}");
    assert_eq!(results.len(), 3);
    assert_eq!(results[2].step, "test");
    assert_eq!(results[2].output, "test");
    std::fs::remove_dir_all(root).unwrap();
}

fn tempdir(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("hardgate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}
