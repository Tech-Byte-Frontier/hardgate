use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const MAX_STREAM_BYTES: usize = 32 * 1024;
const MAX_OUTPUT_BYTES: usize = MAX_STREAM_BYTES * 2;
const OUTPUT_READ_TIMEOUT: Duration = Duration::from_secs(1);
const TERMINATION_GRACE: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub(crate) enum ProcessOutcome {
    Completed { status: ExitStatus, output: String },
    TimedOut { output: String },
    Failed { message: String, output: String },
}

pub(crate) fn run_command(
    tokens: &[String],
    root: &Path,
    timeout: Duration,
    operation: &str,
) -> ProcessOutcome {
    let Some(program) = tokens.first() else {
        return ProcessOutcome::Failed {
            message: "Empty command string; nothing was executed.".to_string(),
            output: String::new(),
        };
    };
    let mut child = match spawn_command(tokens, root) {
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

fn spawn_command(tokens: &[String], root: &Path) -> std::io::Result<Child> {
    let mut command = Command::new(&tokens[0]);
    command
        .args(&tokens[1..])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    prepend_local_bin(&mut command, root);
    configure_process_group(&mut command);
    command.spawn()
}

fn prepend_local_bin(command: &mut Command, root: &Path) {
    let local_bin = root.join("node_modules").join(".bin");
    if !local_bin.is_dir() {
        return;
    }
    let mut paths =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    paths.insert(0, local_bin);
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
        match child.try_wait() {
            Ok(Some(status)) => return ProcessWait::Exited(status),
            Ok(None) if start.elapsed() >= timeout => {
                terminate_process_tree(child);
                return ProcessWait::Timeout;
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                terminate_process_tree(child);
                return ProcessWait::Error(format!(
                    "Failed to wait for {operation} command: {error}"
                ));
            }
        }
    }
}

#[cfg(unix)]
fn terminate_process_tree(child: &mut Child) {
    let pid = child.id().to_string();
    signal_process_group("-TERM", &pid);
    wait_for_termination(child, TERMINATION_GRACE);
    signal_process_group("-KILL", &pid);
    let _ = child.wait();
}

#[cfg(unix)]
fn signal_process_group(signal: &str, pid: &str) {
    let _ = Command::new("kill")
        .args([signal, "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn terminate_process_tree(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_for_termination(child: &mut Child, grace: Duration) {
    let start = Instant::now();
    while start.elapsed() < grace {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}
