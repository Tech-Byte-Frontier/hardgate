#[cfg(unix)]
use std::io;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[path = "process/capture.rs"]
mod capture;
#[path = "process/cleanup.rs"]
mod cleanup;

use capture::{CaptureResult, CapturedOutput};
use cleanup::terminate_process_tree;
pub(crate) use cleanup::timeout_scope;

const MAX_STREAM_BYTES: usize = 32 * 1024;
const MAX_OUTPUT_BYTES: usize = MAX_STREAM_BYTES * 2;
const OUTPUT_READ_TIMEOUT: Duration = Duration::from_secs(1);
const READER_SHUTDOWN_GRACE: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub(crate) enum ProcessOutcome {
    Completed { status: ExitStatus, output: String },
    TimedOut { output: String },
    Failed { message: String, output: String },
}

#[derive(Clone, Copy)]
pub(crate) struct CommandRoots<'a> {
    pub working_dir: &'a Path,
    pub package_root: &'a Path,
    pub workspace_root: &'a Path,
}

impl<'a> CommandRoots<'a> {
    pub(crate) fn single(root: &'a Path) -> Self {
        Self {
            working_dir: root,
            package_root: root,
            workspace_root: root,
        }
    }
}

pub(crate) fn run_command(
    tokens: &[String],
    root: &Path,
    timeout: Duration,
    operation: &str,
) -> ProcessOutcome {
    run_command_with_roots(tokens, CommandRoots::single(root), timeout, operation)
}

pub(crate) fn run_command_with_roots(
    tokens: &[String],
    roots: CommandRoots<'_>,
    timeout: Duration,
    operation: &str,
) -> ProcessOutcome {
    let Some(program) = tokens.first() else {
        return ProcessOutcome::Failed {
            message: "Empty command string; nothing was executed.".to_string(),
            output: String::new(),
        };
    };
    let mut child = match spawn_command(tokens, roots) {
        Ok(child) => child,
        Err(error) => {
            return ProcessOutcome::Failed {
                message: format!("Failed to execute '{program}': {error}"),
                output: String::new(),
            };
        }
    };
    let mut captured = CapturedOutput::from_child(&mut child);
    finish_process_wait(
        wait_for_child(&mut child, timeout, operation),
        &mut child,
        &mut captured,
    )
}

pub(crate) fn append_output(existing: String, extra: String) -> String {
    if extra.is_empty() {
        return truncate_output(existing);
    }
    if existing.is_empty() {
        return truncate_output(extra);
    }
    if extra.len() >= MAX_OUTPUT_BYTES {
        return truncate_output(extra);
    }
    let separator = "\n";
    let existing = truncate_output_to(
        existing,
        MAX_OUTPUT_BYTES.saturating_sub(separator.len() + extra.len()),
    );
    format!("{existing}{separator}{extra}")
}

fn spawn_command(tokens: &[String], roots: CommandRoots<'_>) -> std::io::Result<Child> {
    let mut command = Command::new(&tokens[0]);
    command
        .args(&tokens[1..])
        .current_dir(roots.working_dir)
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    prepend_local_bins(&mut command, roots.package_root, roots.workspace_root);
    configure_process_group(&mut command);
    command.spawn()
}

fn prepend_local_bins(command: &mut Command, package_root: &Path, workspace_root: &Path) {
    let mut local_bins = Vec::new();
    for root in [package_root, workspace_root] {
        let local_bin = root.join("node_modules").join(".bin");
        if local_bin.is_dir()
            && !local_bins
                .iter()
                .any(|item: &std::path::PathBuf| item == &local_bin)
        {
            local_bins.push(local_bin);
        }
    }
    if local_bins.is_empty() {
        return;
    }
    if let Some(path) = compose_path(local_bins, std::env::var_os("PATH")) {
        command.env("PATH", path);
    }
}

fn compose_path(
    local_bins: Vec<std::path::PathBuf>,
    inherited: Option<std::ffi::OsString>,
) -> Option<std::ffi::OsString> {
    let inherited = std::env::split_paths(&inherited.unwrap_or_default())
        .filter(|entry| !local_bins.iter().any(|local| local == entry))
        .collect::<Vec<_>>();
    let mut paths = local_bins;
    paths.extend(inherited);
    std::env::join_paths(paths).ok()
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
macro_rules! declare_fcntl {
    () => {
        unsafe extern "C" {
            fn fcntl(fd: i32, command: i32, ...) -> i32;
        }
    };
}

#[cfg(unix)]
declare_fcntl!();

#[cfg(unix)]
fn set_nonblocking(fd: std::os::fd::RawFd) -> io::Result<()> {
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    const O_NONBLOCK: i32 = 0x0004;
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "freebsd")))]
    const O_NONBLOCK: i32 = 0x0800;

    let flags = unsafe { fcntl(fd, F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn truncate_output(text: String) -> String {
    truncate_output_to(text, MAX_OUTPUT_BYTES)
}

fn truncate_output_to(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

enum ProcessWait {
    Exited(ExitStatus),
    Timeout,
    Error(String),
}

fn finish_process_wait(
    wait: ProcessWait,
    child: &mut Child,
    captured: &mut CapturedOutput,
) -> ProcessOutcome {
    let initial = captured.collect(OUTPUT_READ_TIMEOUT);
    let cleanup_error = cleanup_after_capture(initial.incomplete, &wait, child);
    let capture = finish_capture(captured, initial);
    let reader_error = capture_reader_error(&capture);
    finish_wait_outcome(wait, capture, cleanup_error, reader_error)
}

fn cleanup_after_capture(
    _incomplete: bool,
    wait: &ProcessWait,
    child: &mut Child,
) -> Option<String> {
    // A command may close both capture pipes while descendants continue to
    // run in the inherited process group. Always verify and reap that group,
    // even when the output readers completed before the direct child exited.
    matches!(wait, ProcessWait::Exited(_))
        .then(|| terminate_process_tree(child).err())
        .flatten()
}

fn finish_capture(captured: &mut CapturedOutput, initial: CaptureResult) -> CaptureResult {
    if initial.incomplete {
        captured.cancel();
        captured.collect(READER_SHUTDOWN_GRACE)
    } else {
        initial
    }
}

fn capture_reader_error(capture: &CaptureResult) -> Option<String> {
    capture
        .incomplete
        .then(|| "output readers did not terminate within the bounded cleanup window".to_string())
}

fn finish_wait_outcome(
    wait: ProcessWait,
    capture: CaptureResult,
    cleanup_error: Option<String>,
    reader_error: Option<String>,
) -> ProcessOutcome {
    match wait {
        ProcessWait::Exited(status) => {
            finish_exited(status, capture.output, cleanup_error, reader_error)
        }
        ProcessWait::Timeout => finish_timeout(capture.output, reader_error),
        ProcessWait::Error(message) => finish_wait_error(message, capture.output, reader_error),
    }
}

fn finish_exited(
    status: ExitStatus,
    output: String,
    cleanup_error: Option<String>,
    reader_error: Option<String>,
) -> ProcessOutcome {
    if cleanup_error.is_none() && reader_error.is_none() {
        return ProcessOutcome::Completed { status, output };
    }
    let message = cleanup_error
        .map(|error| {
            format!(
                "command exited, but process cleanup failed while closing inherited pipes: {error}"
            )
        })
        .or(reader_error)
        .unwrap_or_else(|| "command exited but process cleanup failed".to_string());
    ProcessOutcome::Failed { message, output }
}

fn finish_timeout(output: String, reader_error: Option<String>) -> ProcessOutcome {
    match reader_error {
        None => ProcessOutcome::TimedOut { output },
        Some(message) => ProcessOutcome::Failed { message, output },
    }
}

fn finish_wait_error(
    message: String,
    output: String,
    reader_error: Option<String>,
) -> ProcessOutcome {
    let message = reader_error.map_or(message.clone(), |reader| format!("{message}; {reader}"));
    ProcessOutcome::Failed { message, output }
}

fn wait_for_child(child: &mut Child, timeout: Duration, operation: &str) -> ProcessWait {
    let start = Instant::now();
    loop {
        match poll_child(child) {
            ChildPoll::Exited(status) => return ProcessWait::Exited(status),
            ChildPoll::Running if start.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(10));
            }
            ChildPoll::Running => return timeout_process(child, operation),
            ChildPoll::Error(error) => return wait_error_process(child, operation, error),
        }
    }
}

enum ChildPoll {
    Exited(ExitStatus),
    Running,
    Error(std::io::Error),
}

fn poll_child(child: &mut Child) -> ChildPoll {
    match child.try_wait() {
        Ok(Some(status)) => ChildPoll::Exited(status),
        Ok(None) => ChildPoll::Running,
        Err(error) => ChildPoll::Error(error),
    }
}

fn timeout_process(child: &mut Child, operation: &str) -> ProcessWait {
    match terminate_process_tree(child) {
        Ok(()) => ProcessWait::Timeout,
        Err(error) => ProcessWait::Error(format!(
            "{operation} command timed out, but {scope} cleanup failed: {error}",
            scope = timeout_scope()
        )),
    }
}

fn wait_error_process(
    child: &mut Child,
    operation: &str,
    wait_error: std::io::Error,
) -> ProcessWait {
    let cleanup = terminate_process_tree(child);
    let message = match cleanup {
        Ok(()) => format!(
            "Failed to wait for {operation} command: {wait_error}; {scope} cleanup completed",
            scope = timeout_scope()
        ),
        Err(cleanup_error) => format!(
            "Failed to wait for {operation} command: {wait_error}; {scope} cleanup failed: {cleanup_error}",
            scope = timeout_scope()
        ),
    };
    ProcessWait::Error(message)
}

#[cfg(test)]
mod tests {
    use super::compose_path;
    use std::path::PathBuf;

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

        let composed =
            compose_path(vec![package.clone(), workspace.clone()], Some(inherited)).unwrap();
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
}
