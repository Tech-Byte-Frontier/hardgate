use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

#[path = "process/cleanup.rs"]
mod cleanup;

use cleanup::terminate_process_tree;
pub(crate) use cleanup::timeout_scope;

const MAX_STREAM_BYTES: usize = 32 * 1024;
const MAX_OUTPUT_BYTES: usize = MAX_STREAM_BYTES * 2;
const OUTPUT_READ_TIMEOUT: Duration = Duration::from_secs(1);

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
    let captured = CapturedOutput::from_child(&mut child);
    match wait_for_child(&mut child, timeout, operation) {
        ProcessWait::Exited(status) => ProcessOutcome::Completed {
            status,
            output: captured.collect(),
        },
        ProcessWait::Timeout => ProcessOutcome::TimedOut {
            output: captured.collect(),
        },
        ProcessWait::Error(message) => ProcessOutcome::Failed {
            message,
            output: captured.collect(),
        },
    }
}

pub(crate) fn append_output(existing: String, extra: String) -> String {
    let combined = match (existing.is_empty(), extra.is_empty()) {
        (true, _) => extra,
        (_, true) => existing,
        (false, false) => format!("{existing}\n{extra}"),
    };
    truncate_output(combined)
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
    let mut paths = local_bins;
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    if let Ok(path) = std::env::join_paths(paths) {
        command.env("PATH", path);
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

struct CapturedOutput {
    stdout: Option<Receiver<CapturedStream>>,
    stderr: Option<Receiver<CapturedStream>>,
}

impl CapturedOutput {
    fn from_child(child: &mut Child) -> Self {
        Self {
            stdout: child.stdout.take().map(spawn_reader),
            stderr: child.stderr.take().map(spawn_reader),
        }
    }

    fn collect(self) -> String {
        let stdout = receive_stream(self.stdout);
        let stderr = receive_stream(self.stderr);
        combine_streams(stdout, stderr)
    }
}

#[derive(Debug)]
struct CapturedStream {
    bytes: Vec<u8>,
    truncated: bool,
}

fn spawn_reader<R>(mut reader: R) -> Receiver<CapturedStream>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::with_capacity(MAX_STREAM_BYTES);
        let mut truncated = false;
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => append_chunk(&mut bytes, &mut truncated, &buffer[..read]),
                Err(_) => break,
            }
        }
        let _ = sender.send(CapturedStream { bytes, truncated });
    });
    receiver
}

fn append_chunk(bytes: &mut Vec<u8>, truncated: &mut bool, chunk: &[u8]) {
    let remaining = MAX_STREAM_BYTES.saturating_sub(bytes.len());
    let keep = chunk.len().min(remaining);
    bytes.extend_from_slice(&chunk[..keep]);
    *truncated |= keep < chunk.len();
}

fn receive_stream(receiver: Option<Receiver<CapturedStream>>) -> CapturedStream {
    receiver
        .and_then(|channel| channel.recv_timeout(OUTPUT_READ_TIMEOUT).ok())
        .unwrap_or(CapturedStream {
            bytes: Vec::new(),
            truncated: false,
        })
}

fn combine_streams(stdout: CapturedStream, stderr: CapturedStream) -> String {
    let stdout = stream_text(stdout);
    let stderr = stream_text(stderr);
    let combined = match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
    };
    truncate_output(combined)
}

fn stream_text(stream: CapturedStream) -> String {
    let mut text = String::from_utf8_lossy(&stream.bytes).trim().to_string();
    if stream.truncated {
        text.push_str("\n[output truncated after 32768 bytes]");
    }
    text
}

fn truncate_output(mut text: String) -> String {
    if text.len() <= MAX_OUTPUT_BYTES {
        return text;
    }
    let mut end = MAX_OUTPUT_BYTES;
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
