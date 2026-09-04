use super::runner::MutantOutcome;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const MAX_STREAM_BYTES: usize = 32 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = MAX_STREAM_BYTES * 2;
const READER_TIMEOUT: Duration = Duration::from_secs(1);
const TERMINATION_GRACE: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub(crate) struct CommandExecution {
    pub outcome: MutantOutcome,
    pub diagnostic: String,
}

pub(crate) fn execute_with_timeout(
    command: &str,
    root: &std::path::Path,
    timeout_secs: u64,
) -> CommandExecution {
    let tokens = crate::engines::orchestration::shell_words_split(command);
    let Some(program) = tokens.first() else {
        return command_error("Empty command string".to_string());
    };
    let mut child = match spawn_command(&tokens, root) {
        Ok(child) => child,
        Err(error) => {
            return command_error(format!("Failed to execute '{program}': {error}"));
        }
    };
    let output = CapturedOutput::from_child(&mut child);
    let wait = wait_for_child(&mut child, Duration::from_secs(timeout_secs.max(1)));
    let diagnostic = output.collect();
    finish_wait(wait, diagnostic)
}

fn command_error(diagnostic: String) -> CommandExecution {
    CommandExecution {
        outcome: MutantOutcome::RunnerError,
        diagnostic,
    }
}

fn spawn_command(tokens: &[String], root: &std::path::Path) -> std::io::Result<Child> {
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

fn prepend_local_bin(command: &mut Command, root: &std::path::Path) {
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
    stdout: Option<Receiver<Vec<u8>>>,
    stderr: Option<Receiver<Vec<u8>>>,
}

impl CapturedOutput {
    fn from_child(child: &mut Child) -> Self {
        Self {
            stdout: child.stdout.take().map(spawn_bounded_reader),
            stderr: child.stderr.take().map(spawn_bounded_reader),
        }
    }

    fn collect(self) -> String {
        let stdout = receive_output(self.stdout);
        let stderr = receive_output(self.stderr);
        combine_diagnostics(&stdout, &stderr)
    }
}

fn spawn_bounded_reader<R>(reader: R) -> Receiver<Vec<u8>>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let retained = read_bounded(reader);
        let _ = sender.send(retained);
    });
    receiver
}

fn read_bounded<R>(mut reader: R) -> Vec<u8>
where
    R: Read,
{
    let mut retained = Vec::with_capacity(MAX_STREAM_BYTES);
    let mut buffer = [0_u8; 4096];
    while let Ok(read) = reader.read(&mut buffer) {
        if read == 0 {
            break;
        }
        append_bounded(&mut retained, &buffer[..read]);
    }
    retained
}

fn append_bounded(retained: &mut Vec<u8>, chunk: &[u8]) {
    let remaining = MAX_STREAM_BYTES.saturating_sub(retained.len());
    retained.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
}

fn receive_output(receiver: Option<Receiver<Vec<u8>>>) -> Vec<u8> {
    receiver
        .and_then(|channel| channel.recv_timeout(READER_TIMEOUT).ok())
        .unwrap_or_default()
}

fn combine_diagnostics(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let combined = match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
    };
    truncate_diagnostic(combined)
}

fn truncate_diagnostic(mut text: String) -> String {
    if text.len() <= MAX_DIAGNOSTIC_BYTES {
        return text;
    }
    let mut end = MAX_DIAGNOSTIC_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

fn finish_wait(wait: ProcessWait, diagnostic: String) -> CommandExecution {
    match wait {
        ProcessWait::Exited(status) => CommandExecution {
            outcome: outcome_from_status(&status, &diagnostic),
            diagnostic,
        },
        ProcessWait::Timeout => CommandExecution {
            outcome: MutantOutcome::Timeout,
            diagnostic,
        },
        ProcessWait::RunnerError(error) => CommandExecution {
            outcome: MutantOutcome::RunnerError,
            diagnostic: append_text(diagnostic, error),
        },
    }
}

enum ProcessWait {
    Exited(ExitStatus),
    Timeout,
    RunnerError(String),
}

fn wait_for_child(child: &mut Child, max_duration: Duration) -> ProcessWait {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return ProcessWait::Exited(status),
            Ok(None) if start.elapsed() >= max_duration => {
                terminate_process_tree(child);
                return ProcessWait::Timeout;
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                terminate_process_tree(child);
                return ProcessWait::RunnerError(format!(
                    "Failed to wait for mutation command: {error}"
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

fn outcome_from_status(status: &ExitStatus, diagnostic: &str) -> MutantOutcome {
    if status.success() {
        MutantOutcome::Survived
    } else if looks_like_compile_error(diagnostic) {
        MutantOutcome::CompileError
    } else {
        MutantOutcome::Killed
    }
}

fn looks_like_compile_error(diagnostic: &str) -> bool {
    let lower = diagnostic.to_ascii_lowercase();
    [
        "could not compile",
        "compilation failed",
        "compile error",
        "compile_error",
        "syntaxerror",
        "syntax error",
        "failed to parse",
        "error[e",
        "error ts",
        "typecheck failed",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn append_text(existing: String, extra: String) -> String {
    let combined = match (existing.is_empty(), extra.is_empty()) {
        (true, _) => extra,
        (_, true) => existing,
        (false, false) => format!("{existing}\n{extra}"),
    };
    truncate_diagnostic(combined)
}
