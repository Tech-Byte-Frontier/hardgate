use super::generator::AstMutant;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutantOutcome {
    Killed,
    Survived,
    Timeout,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutantExecutionResult {
    pub mutant: AstMutant,
    pub outcome: MutantOutcome,
    pub duration_ms: u128,
    pub command: String,
}

pub struct NativeMutationRunner {
    timeout_secs: u64,
    test_cmd: Option<String>,
}

struct RollbackGuard<'a> {
    file_path: &'a Path,
    original_bytes: &'a [u8],
}

impl<'a> Drop for RollbackGuard<'a> {
    fn drop(&mut self) {
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

    pub fn run_mutant(&self, mutant: &AstMutant, root: &Path) -> MutantExecutionResult {
        let start = Instant::now();
        let target_path = if mutant.file.is_absolute() {
            mutant.file.clone()
        } else {
            root.join(&mutant.file)
        };

        let Ok(original_bytes) = fs::read(&target_path) else {
            return MutantExecutionResult {
                mutant: mutant.clone(),
                outcome: MutantOutcome::Error,
                duration_ms: 0,
                command: String::new(),
            };
        };

        let cmd_str = self.resolve_test_command(&mutant.file, root);

        // Scope the file modification with RAII rollback guard
        let outcome = {
            let _guard = RollbackGuard {
                file_path: &target_path,
                original_bytes: &original_bytes,
            };

            if apply_mutant_bytes(&target_path, mutant, &original_bytes).is_err() {
                return MutantExecutionResult {
                    mutant: mutant.clone(),
                    outcome: MutantOutcome::Error,
                    duration_ms: 0,
                    command: cmd_str,
                };
            }

            self.execute_test_with_timeout(&cmd_str, root)
        };

        let duration_ms = start.elapsed().as_millis();
        MutantExecutionResult {
            mutant: mutant.clone(),
            outcome,
            duration_ms,
            command: cmd_str,
        }
    }

    fn resolve_test_command(&self, file: &Path, root: &Path) -> String {
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

    fn execute_test_with_timeout(&self, cmd_str: &str, root: &Path) -> MutantOutcome {
        let tokens = crate::engines::orchestration::shell_words_split(cmd_str);
        if tokens.is_empty() {
            return MutantOutcome::Error;
        }

        let Ok(mut child) = spawn_quiet(&tokens, root) else {
            return MutantOutcome::Error;
        };

        wait_for_child(&mut child, Duration::from_secs(self.timeout_secs))
    }
}

fn apply_mutant_bytes(
    target_path: &Path,
    mutant: &AstMutant,
    original_bytes: &[u8],
) -> std::io::Result<()> {
    if mutant.start_byte > original_bytes.len() || mutant.end_byte > original_bytes.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "mutant byte range out of bounds",
        ));
    }
    let mut mutated_bytes = Vec::new();
    mutated_bytes.extend_from_slice(&original_bytes[..mutant.start_byte]);
    mutated_bytes.extend_from_slice(mutant.replacement.as_bytes());
    mutated_bytes.extend_from_slice(&original_bytes[mutant.end_byte..]);
    fs::write(target_path, &mutated_bytes)
}

fn spawn_quiet(tokens: &[String], root: &Path) -> std::io::Result<std::process::Child> {
    let mut cmd = Command::new(&tokens[0]);
    cmd.args(&tokens[1..]);
    cmd.current_dir(root);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.spawn()
}

fn wait_for_child(child: &mut std::process::Child, max_duration: Duration) -> MutantOutcome {
    let start = Instant::now();
    loop {
        if let Some(outcome) = poll_child_once(child, start, max_duration) {
            return outcome;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

enum PollAction {
    Done(MutantOutcome),
    Waiting,
}

fn poll_child_once(
    child: &mut std::process::Child,
    start: Instant,
    max_duration: Duration,
) -> Option<MutantOutcome> {
    match poll_action(child, start, max_duration) {
        PollAction::Done(outcome) => Some(outcome),
        PollAction::Waiting => None,
    }
}

fn poll_action(
    child: &mut std::process::Child,
    start: Instant,
    max_duration: Duration,
) -> PollAction {
    let waited = child.try_wait();
    map_wait_result(waited, child, start, max_duration)
}

fn map_wait_result(
    waited: std::io::Result<Option<std::process::ExitStatus>>,
    child: &mut std::process::Child,
    start: Instant,
    max_duration: Duration,
) -> PollAction {
    let Ok(opt) = waited else {
        return PollAction::Done(MutantOutcome::Error);
    };
    let Some(status) = opt else {
        return map_pending(child, start, max_duration);
    };
    PollAction::Done(outcome_from_status(&status))
}

fn map_pending(
    child: &mut std::process::Child,
    start: Instant,
    max_duration: Duration,
) -> PollAction {
    if let Some(outcome) = check_timeout(child, start, max_duration) {
        PollAction::Done(outcome)
    } else {
        PollAction::Waiting
    }
}

fn check_timeout(
    child: &mut std::process::Child,
    start: Instant,
    max_duration: Duration,
) -> Option<MutantOutcome> {
    if start.elapsed() > max_duration {
        let _ = child.kill();
        let _ = child.wait();
        Some(MutantOutcome::Timeout)
    } else {
        None
    }
}

fn outcome_from_status(status: &std::process::ExitStatus) -> MutantOutcome {
    if status.success() {
        MutantOutcome::Survived
    } else {
        MutantOutcome::Killed
    }
}

fn resolve_rust_test_cmd(stem: &str) -> String {
    if !stem.is_empty() && stem != "main" && stem != "lib" && stem != "mod" {
        format!("cargo test {}", stem)
    } else {
        "cargo test".to_string()
    }
}

fn resolve_js_test_cmd(file: &Path, root: &Path, stem: &str, ext: &str) -> String {
    let candidates = [
        format!("{}.test.{}", stem, ext),
        format!("{}.spec.{}", stem, ext),
    ];
    for cand in &candidates {
        if root.join("tests").join(cand).exists() || file.with_file_name(cand).exists() {
            return format!("pnpm test {}", cand);
        }
    }
    "pnpm test".to_string()
}
