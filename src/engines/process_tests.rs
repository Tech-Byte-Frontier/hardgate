use super::{
    CaptureResult, CommandRoots, ProcessOutcome, ProcessWait, append_output, compose_path,
    finish_exited, finish_timeout, finish_wait_error, finish_wait_outcome, run_command_with_roots,
    truncate_output_to,
};
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn local_bins_precede_and_filter_inherited_duplicates() {
    let package = PathBuf::from("/workspace/package/node_modules/.bin");
    let workspace = PathBuf::from("/workspace/node_modules/.bin");
    let inherited = std::env::join_paths([
        package.clone(),
        PathBuf::from("/usr/bin"),
        workspace.clone(),
        PathBuf::from("/bin"),
    ])
    .unwrap();

    let composed = compose_path(vec![package.clone(), workspace.clone()], Some(inherited)).unwrap();
    let entries = std::env::split_paths(&composed).collect::<Vec<_>>();

    assert_eq!(
        entries,
        vec![
            package,
            workspace,
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin")
        ]
    );
}

#[test]
fn empty_command_is_rejected_without_spawning() {
    let outcome = run_command_with_roots(
        &[],
        CommandRoots::single(std::path::Path::new(".")),
        Duration::from_secs(1),
        "test",
    );

    assert!(matches!(
        outcome,
        ProcessOutcome::Failed { message, output }
            if message == "Empty command string; nothing was executed." && output.is_empty()
    ));
}

#[test]
fn append_output_preserves_empty_and_caps_large_extra() {
    assert_eq!(
        append_output("existing".to_string(), String::new()),
        "existing"
    );

    let extra = "x".repeat(super::MAX_OUTPUT_BYTES);
    let output = append_output("prefix".to_string(), extra);
    assert_eq!(output.len(), super::MAX_OUTPUT_BYTES);
    assert!(output.chars().all(|character| character == 'x'));
}

#[test]
fn truncate_output_stops_at_unicode_boundary() {
    assert_eq!(truncate_output_to("abc🙂".to_string(), 4), "abc");
}

#[test]
fn wait_error_outcome_preserves_capture_and_reader_error() {
    let capture = CaptureResult {
        output: "captured".to_string(),
        incomplete: false,
    };
    let outcome = finish_wait_outcome(
        ProcessWait::Error("wait failed".to_string()),
        capture,
        None,
        None,
    );

    assert!(matches!(
        outcome,
        ProcessOutcome::Failed { message, output }
            if message == "wait failed" && output == "captured"
    ));

    let outcome = finish_wait_error(
        "wait failed".to_string(),
        "captured".to_string(),
        Some("reader failed".to_string()),
    );
    assert!(matches!(
        outcome,
        ProcessOutcome::Failed { message, output }
            if message == "wait failed; reader failed" && output == "captured"
    ));
}

#[cfg(unix)]
#[test]
fn exited_outcome_reports_cleanup_and_reader_errors() {
    use std::os::unix::process::ExitStatusExt;

    let status = std::process::ExitStatus::from_raw(0);
    let outcome = finish_exited(
        status,
        "captured".to_string(),
        Some("descendant remained".to_string()),
        None,
    );
    assert!(matches!(
        outcome,
        ProcessOutcome::Failed { message, output }
            if message == "command exited, but process cleanup failed while closing inherited pipes: descendant remained"
                && output == "captured"
    ));

    let status = std::process::ExitStatus::from_raw(0);
    let outcome = finish_exited(
        status,
        "captured".to_string(),
        None,
        Some("reader failed".to_string()),
    );
    assert!(matches!(
        outcome,
        ProcessOutcome::Failed { message, output }
            if message == "reader failed" && output == "captured"
    ));
}

#[test]
fn timeout_outcome_reports_reader_error() {
    let outcome = finish_timeout("captured".to_string(), Some("reader failed".to_string()));
    assert!(matches!(
        outcome,
        ProcessOutcome::Failed { message, output }
            if message == "reader failed" && output == "captured"
    ));
}

#[cfg(unix)]
#[test]
fn invalid_capture_fd_reports_nonblocking_error() {
    assert!(super::set_nonblocking(-1).is_err());
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn unsupported_group_timeout_evidence_is_direct_child_only() {
    let evidence = super::timeout_cleanup_evidence();
    assert!(evidence.contains("direct child"));
    assert!(evidence.contains("reaped"));
    assert!(!evidence.contains("absence was verified"));
    assert!(!evidence.contains("process group"));
}
