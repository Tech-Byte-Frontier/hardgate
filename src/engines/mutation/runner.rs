use super::generator::AstMutant;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
/// Maximum amount of output retained from each command stream.
///
/// Mutation commands can be noisy (especially compilers). Keeping the streams
/// bounded prevents a failed mutant from consuming unbounded memory while
/// still retaining enough context to classify compile failures.
const MAX_DIAGNOSTIC_STREAM_BYTES: usize = 32 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = MAX_DIAGNOSTIC_STREAM_BYTES * 2;
const DIAGNOSTIC_READER_TIMEOUT: Duration = Duration::from_secs(1);
const TERMINATION_GRACE: Duration = Duration::from_millis(200);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutantOutcome {
    Killed,
    Survived,
    CompileError,
    RunnerError,
    Timeout,
    Equivalent,
    Unviable,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutantExecutionResult {
    pub mutant: AstMutant,
    pub outcome: MutantOutcome,
    pub duration_ms: u128,
    pub command: String,
    /// Bounded stdout/stderr from the test command or a runner diagnostic.
    pub diagnostic: String,
    /// Whether the original source bytes were restored and verified.
    pub source_restored: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaselineOutcome {
    Passed,
    Failed,
    Timeout,
    RunnerError,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineExecutionResult {
    pub file: PathBuf,
    pub outcome: BaselineOutcome,
    pub duration_ms: u128,
    pub command: String,
    /// Bounded stdout/stderr from the baseline command or a runner diagnostic.
    pub diagnostic: String,
}
pub struct NativeMutationRunner {
    timeout_secs: u64,
    test_cmd: Option<String>,
}
struct RollbackGuard<'a> {
    file_path: &'a Path,
    original_bytes: &'a [u8],
    restored: bool,
}
impl<'a> RollbackGuard<'a> {
    fn new(file_path: &'a Path, original_bytes: &'a [u8]) -> Self {
        Self {
            file_path,
            original_bytes,
            restored: false,
        }
    }
    /// Restore the source and verify byte-for-byte equality.
    fn restore(&mut self) -> std::io::Result<()> {
        fs::write(self.file_path, self.original_bytes)?;
        let restored = fs::read(self.file_path)?;
        if restored != self.original_bytes {
            return Err(std::io::Error::other(
                "restored source bytes differ from the original",
            ));
        }
        self.restored = true;
        Ok(())
    }
}
impl<'a> Drop for RollbackGuard<'a> {
    fn drop(&mut self) {
        if self.restored {
            return;
        }
        if let Err(e) = fs::write(self.file_path, self.original_bytes) {
            eprintln!(
                "hardgate: failed to restore {} after mutation: {}",
                self.file_path.display(),
                e
            );
        }
    }
}
impl NativeMutationRunner {
    pub fn new(timeout_secs: u64, test_cmd: Option<String>) -> Self {
        Self {
            timeout_secs,
            test_cmd,
        }
    }
    /// Run one mutant and restore the source before returning.
    pub fn run_mutant(&self, mutant: &AstMutant, root: &Path) -> MutantExecutionResult {
        let start = Instant::now();
        let target_path = resolve_target_path(&mutant.file, root);
        let command = self.resolve_test_command(&mutant.file, root);
        let original_bytes = match fs::read(&target_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return MutantExecutionResult {
                    mutant: mutant.clone(),
                    outcome: MutantOutcome::RunnerError,
                    duration_ms: start.elapsed().as_millis(),
                    command,
                    diagnostic: format!(
                        "Failed to read mutation target '{}': {error}",
                        target_path.display()
                    ),
                    source_restored: false,
                };
            }
        };
        let mut guard = RollbackGuard::new(&target_path, &original_bytes);
        let mut execution = match apply_mutant_bytes(&target_path, mutant, &original_bytes) {
            Ok(()) => self.execute_test_with_timeout(&command, root),
            Err(error) => CommandExecution {
                outcome: if error.kind() == std::io::ErrorKind::InvalidInput {
                    MutantOutcome::Unviable
                } else {
                    MutantOutcome::RunnerError
                },
                diagnostic: format!("Failed to apply mutant {}: {error}", mutant.id),
            },
        };
        let source_restored = guard.restore().is_ok();
        if !source_restored {
            execution.outcome = MutantOutcome::RunnerError;
            append_diagnostic(
                &mut execution.diagnostic,
                format!(
                    "Failed to restore and verify original source '{}'.",
                    target_path.display()
                ),
            );
        }
        drop(guard);

        MutantExecutionResult {
            mutant: mutant.clone(),
            outcome: execution.outcome,
            duration_ms: start.elapsed().as_millis(),
            command,
            diagnostic: execution.diagnostic,
            source_restored,
        }
    }
    /// Run the resolved test command against the unmodified source tree.
    pub fn run_baseline(&self, file: &Path, root: &Path) -> BaselineExecutionResult {
        let start = Instant::now();
        let command = self.resolve_test_command(file, root);
        let execution = self.execute_test_with_timeout(&command, root);
        let outcome = match execution.outcome {
            MutantOutcome::Survived => BaselineOutcome::Passed,
            MutantOutcome::Timeout => BaselineOutcome::Timeout,
            MutantOutcome::RunnerError => BaselineOutcome::RunnerError,
            MutantOutcome::Killed
            | MutantOutcome::CompileError
            | MutantOutcome::Equivalent
            | MutantOutcome::Unviable => BaselineOutcome::Failed,
        };

        BaselineExecutionResult {
            file: file.to_path_buf(),
            outcome,
            duration_ms: start.elapsed().as_millis(),
            command,
            diagnostic: execution.diagnostic,
        }
    }
    pub(crate) fn resolve_test_command(&self, file: &Path, root: &Path) -> String {
        if let Some(ref cmd) = self.test_cmd {
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            return cmd
                .replace("{file}", &file.to_string_lossy())
                .replace("{stem}", stem);
        }

        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        match ext {
            "rs" => resolve_rust_test_cmd(stem),
            "ts" | "tsx" | "js" | "jsx" => resolve_js_test_cmd(file, root, stem, ext),
            _ => "cargo test".to_string(),
        }
    }
    fn execute_test_with_timeout(&self, cmd_str: &str, root: &Path) -> CommandExecution {
        let tokens = crate::engines::orchestration::shell_words_split(cmd_str);
        if tokens.is_empty() {
            return CommandExecution {
                outcome: MutantOutcome::RunnerError,
                diagnostic: "Empty command string".to_string(),
            };
        }
        let mut child = match spawn_quiet(&tokens, root) {
            Ok(child) => child,
            Err(error) => {
                return CommandExecution {
                    outcome: MutantOutcome::RunnerError,
                    diagnostic: format!("Failed to execute '{}': {error}", tokens[0]),
                };
            }
        };

        let output = CapturedOutput::from_child(&mut child);
        let wait = wait_for_child(&mut child, Duration::from_secs(self.timeout_secs));
        let diagnostic = output.collect();

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
}

#[derive(Debug)]
struct CommandExecution {
    outcome: MutantOutcome,
    diagnostic: String,
}

fn resolve_target_path(file: &Path, root: &Path) -> PathBuf {
    if file.is_absolute() {
        file.to_path_buf()
    } else {
        root.join(file)
    }
}

fn apply_mutant_bytes(
    target_path: &Path,
    mutant: &AstMutant,
    original_bytes: &[u8],
) -> std::io::Result<()> {
    if mutant.start_byte > mutant.end_byte || mutant.end_byte > original_bytes.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "mutant byte range out of bounds",
        ));
    }
    if &original_bytes[mutant.start_byte..mutant.end_byte] != mutant.original.as_bytes() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "mutant original text does not match the source bytes",
        ));
    }

    let mut mutated_bytes = Vec::with_capacity(
        original_bytes.len() - (mutant.end_byte - mutant.start_byte) + mutant.replacement.len(),
    );
    mutated_bytes.extend_from_slice(&original_bytes[..mutant.start_byte]);
    mutated_bytes.extend_from_slice(mutant.replacement.as_bytes());
    mutated_bytes.extend_from_slice(&original_bytes[mutant.end_byte..]);
    fs::write(target_path, &mutated_bytes)
}

fn spawn_quiet(tokens: &[String], root: &Path) -> std::io::Result<Child> {
    let mut cmd = Command::new(&tokens[0]);
    cmd.args(&tokens[1..]);
    cmd.current_dir(root);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    configure_process_group(&mut cmd);
    cmd.spawn()
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
        combine_diagnostic_streams(&stdout, &stderr)
    }
}

fn spawn_bounded_reader<R>(mut reader: R) -> Receiver<Vec<u8>>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut retained = Vec::with_capacity(MAX_DIAGNOSTIC_STREAM_BYTES);
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    let remaining = MAX_DIAGNOSTIC_STREAM_BYTES.saturating_sub(retained.len());
                    if remaining > 0 {
                        retained.extend_from_slice(&buffer[..read.min(remaining)]);
                    }
                }
                Err(_) => break,
            }
        }
        let _ = sender.send(retained);
    });
    receiver
}

fn receive_output(receiver: Option<Receiver<Vec<u8>>>) -> Vec<u8> {
    receiver
        .and_then(|channel| channel.recv_timeout(DIAGNOSTIC_READER_TIMEOUT).ok())
        .unwrap_or_default()
}

fn combine_diagnostic_streams(stdout: &[u8], stderr: &[u8]) -> String {
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

fn append_text(existing: String, extra: String) -> String {
    let combined = if existing.is_empty() {
        extra
    } else if extra.is_empty() {
        existing
    } else {
        format!("{existing}\n{extra}")
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

fn append_diagnostic(existing: &mut String, extra: String) {
    *existing = append_text(std::mem::take(existing), extra);
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

fn resolve_rust_test_cmd(stem: &str) -> String {
    if !stem.is_empty() && stem != "main" && stem != "lib" && stem != "mod" {
        format!("cargo test {stem}")
    } else {
        "cargo test".to_string()
    }
}

fn resolve_js_test_cmd(file: &Path, root: &Path, stem: &str, ext: &str) -> String {
    let candidates = [format!("{stem}.test.{ext}"), format!("{stem}.spec.{ext}")];
    for candidate in &candidates {
        if root.join("tests").join(candidate).exists() || file.with_file_name(candidate).exists() {
            return format!("pnpm test {candidate}");
        }
    }
    "pnpm test".to_string()
}
