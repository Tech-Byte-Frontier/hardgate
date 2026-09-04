use super::generator::AstMutant;
use super::js::{ResolvedTestPlan, TestSelection, is_javascript_path, resolve_js_test_plan};
use super::process::{CommandExecution, execute_with_timeout};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Maximum timeout used when a JavaScript project has no trustworthy file
/// mapping and the complete test suite must run.
pub const FULL_SUITE_TIMEOUT_SECS: u64 = 60;
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;

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

    fn restore(&mut self) -> std::io::Result<()> {
        fs::write(self.file_path, self.original_bytes)?;
        if fs::read(self.file_path)? != self.original_bytes {
            return Err(std::io::Error::other(
                "restored source bytes differ from the original",
            ));
        }
        self.restored = true;
        Ok(())
    }
}

impl Drop for RollbackGuard<'_> {
    fn drop(&mut self) {
        if self.restored {
            return;
        }
        if let Err(error) = fs::write(self.file_path, self.original_bytes) {
            eprintln!(
                "hardgate: failed to restore {} after mutation: {}",
                self.file_path.display(),
                error
            );
        }
    }
}

impl NativeMutationRunner {
    pub fn new(timeout_secs: u64, test_cmd: Option<String>) -> Self {
        Self {
            timeout_secs: timeout_secs.max(1),
            test_cmd,
        }
    }

    /// Resolve command, package manager, framework, workspace root, and test
    /// selection. The metadata makes a full-suite fallback explicit to callers.
    pub fn resolve_test_plan(&self, file: &Path, root: &Path) -> ResolvedTestPlan {
        if let Some(command) = self.test_cmd.as_deref() {
            return custom_plan(command, file, root);
        }
        if is_javascript_path(file) {
            return resolve_js_test_plan(file, root);
        }
        if file.extension().and_then(|value| value.to_str()) == Some("rs") {
            rust_plan(file, root)
        } else {
            plain_plan("cargo test".to_string(), root, TestSelection::Custom)
        }
    }

    pub fn resolve_test_command(&self, file: &Path, root: &Path) -> String {
        self.resolve_test_plan(file, root).command
    }

    /// Pick a safe default timeout when an automatically resolved JS command
    /// has to execute an entire suite. Explicit user/configured values win.
    pub fn default_timeout_secs(files: &[PathBuf], root: &Path, test_cmd: Option<&str>) -> u64 {
        if test_cmd.is_none()
            && files.iter().any(|file| {
                is_javascript_path(file)
                    && resolve_js_test_plan(file, root).selection.is_full_suite()
            })
        {
            FULL_SUITE_TIMEOUT_SECS
        } else {
            DEFAULT_TIMEOUT_SECS
        }
    }

    /// Run one mutant and restore the source before returning.
    pub fn run_mutant(&self, mutant: &AstMutant, root: &Path) -> MutantExecutionResult {
        let start = Instant::now();
        let target_path = resolve_target_path(&mutant.file, root);
        let plan = self.resolve_test_plan(&mutant.file, root);
        let original_bytes = match fs::read(&target_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return failed_mutant(
                    mutant,
                    &plan.command,
                    start,
                    format!(
                        "Failed to read mutation target '{}': {error}",
                        target_path.display()
                    ),
                );
            }
        };
        let mut guard = RollbackGuard::new(&target_path, &original_bytes);
        let mut execution = apply_and_execute(
            self,
            MutationInput {
                mutant,
                target_path: &target_path,
                original_bytes: &original_bytes,
                plan: &plan,
            },
        );
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
            command: plan.command,
            diagnostic: execution.diagnostic,
            source_restored,
        }
    }

    /// Run the resolved test command against the unmodified source tree.
    pub fn run_baseline(&self, file: &Path, root: &Path) -> BaselineExecutionResult {
        let start = Instant::now();
        let plan = self.resolve_test_plan(file, root);
        let execution = execute_with_timeout(&plan.command, &plan.working_dir, self.timeout_secs);
        BaselineExecutionResult {
            file: file.to_path_buf(),
            outcome: baseline_outcome(execution.outcome),
            duration_ms: start.elapsed().as_millis(),
            command: plan.command,
            diagnostic: baseline_diagnostic(execution.diagnostic, &plan.selection),
        }
    }
}

fn custom_plan(command: &str, file: &Path, root: &Path) -> ResolvedTestPlan {
    let stem = file
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("");
    let command = command
        .replace("{file}", &file.to_string_lossy())
        .replace("{stem}", stem);
    plain_plan(command, root, TestSelection::Custom)
}

fn rust_plan(file: &Path, root: &Path) -> ResolvedTestPlan {
    let stem = file
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("");
    let command = if stem.is_empty() || matches!(stem, "main" | "lib" | "mod") {
        "cargo test".to_string()
    } else {
        format!("cargo test {stem}")
    };
    plain_plan(command, root, TestSelection::Custom)
}

fn plain_plan(command: String, root: &Path, selection: TestSelection) -> ResolvedTestPlan {
    ResolvedTestPlan {
        command,
        working_dir: root.to_path_buf(),
        package_root: root.to_path_buf(),
        workspace_root: root.to_path_buf(),
        manager: None,
        framework: None,
        selection,
        recommended_timeout_secs: DEFAULT_TIMEOUT_SECS,
    }
}

struct MutationInput<'a> {
    mutant: &'a AstMutant,
    target_path: &'a Path,
    original_bytes: &'a [u8],
    plan: &'a ResolvedTestPlan,
}

fn apply_and_execute(runner: &NativeMutationRunner, input: MutationInput<'_>) -> CommandExecution {
    match apply_mutant_bytes(input.target_path, input.mutant, input.original_bytes) {
        Ok(()) => execute_with_timeout(
            &input.plan.command,
            &input.plan.working_dir,
            runner.timeout_secs,
        ),
        Err(error) => CommandExecution {
            outcome: if error.kind() == std::io::ErrorKind::InvalidInput {
                MutantOutcome::Unviable
            } else {
                MutantOutcome::RunnerError
            },
            diagnostic: format!("Failed to apply mutant {}: {error}", input.mutant.id),
        },
    }
}

fn failed_mutant(
    mutant: &AstMutant,
    command: &str,
    start: Instant,
    diagnostic: String,
) -> MutantExecutionResult {
    MutantExecutionResult {
        mutant: mutant.clone(),
        outcome: MutantOutcome::RunnerError,
        duration_ms: start.elapsed().as_millis(),
        command: command.to_string(),
        diagnostic,
        source_restored: false,
    }
}

fn baseline_outcome(outcome: MutantOutcome) -> BaselineOutcome {
    match outcome {
        MutantOutcome::Survived => BaselineOutcome::Passed,
        MutantOutcome::Timeout => BaselineOutcome::Timeout,
        MutantOutcome::RunnerError => BaselineOutcome::RunnerError,
        MutantOutcome::Killed
        | MutantOutcome::CompileError
        | MutantOutcome::Equivalent
        | MutantOutcome::Unviable => BaselineOutcome::Failed,
    }
}

fn baseline_diagnostic(mut diagnostic: String, selection: &TestSelection) -> String {
    if selection.is_full_suite() {
        let note = "full suite selected: no reliable relevant test was found";
        if diagnostic.is_empty() {
            return note.to_string();
        }
        diagnostic.push('\n');
        diagnostic.push_str(note);
    }
    diagnostic
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
    let mut mutated = Vec::with_capacity(
        original_bytes.len() - (mutant.end_byte - mutant.start_byte) + mutant.replacement.len(),
    );
    mutated.extend_from_slice(&original_bytes[..mutant.start_byte]);
    mutated.extend_from_slice(mutant.replacement.as_bytes());
    mutated.extend_from_slice(&original_bytes[mutant.end_byte..]);
    fs::write(target_path, mutated)
}

fn append_diagnostic(existing: &mut String, extra: String) {
    if existing.is_empty() {
        existing.push_str(&extra);
    } else {
        existing.push('\n');
        existing.push_str(&extra);
    }
}
