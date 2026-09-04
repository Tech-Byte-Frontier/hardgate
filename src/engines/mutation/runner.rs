use super::generator::AstMutant;
use super::js::{ResolvedTestPlan, TestSelection, is_javascript_path, resolve_js_test_plan};
use super::process::{CommandExecution, baseline_outcome, execute_with_timeout};
use crate::engines::process::{CommandRoots, append_output};
#[path = "restore.rs"]
mod restore;
use restore::{
    SourceSnapshot, atomic_replace, ensure_regular_file, restore_and_verify, snapshot_regular_file,
    verify_and_restore,
};
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
    root: &'a Path,
    original_bytes: &'a [u8],
    original_permissions: std::fs::Permissions,
    restored: bool,
}

impl<'a> RollbackGuard<'a> {
    fn new(
        file_path: &'a Path,
        root: &'a Path,
        original_bytes: &'a [u8],
        original_permissions: std::fs::Permissions,
    ) -> Self {
        Self {
            file_path,
            root,
            original_bytes,
            original_permissions,
            restored: false,
        }
    }

    fn restore(&mut self) -> std::io::Result<()> {
        let result = restore_and_verify(
            self.file_path,
            self.root,
            self.original_bytes,
            &self.original_permissions,
        );
        if result.is_ok() {
            self.restored = true;
        }
        result
    }
}

impl Drop for RollbackGuard<'_> {
    fn drop(&mut self) {
        if self.restored {
            return;
        }
        match restore_and_verify(
            self.file_path,
            self.root,
            self.original_bytes,
            &self.original_permissions,
        ) {
            Ok(()) => eprintln!(
                "hardgate: restored and verified {} after mutation during cleanup",
                self.file_path.display()
            ),
            Err(error) => eprintln!(
                "hardgate: failed to restore and verify {} after mutation: {}",
                self.file_path.display(),
                error
            ),
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

    fn full_suite_timeout_error(&self, plan: &ResolvedTestPlan) -> Option<String> {
        (plan.full_suite_timeout_required() && self.timeout_secs < plan.recommended_timeout_secs)
            .then(|| {
                format!(
                    "JavaScript full-suite test execution requires timeout_secs >= {}s (configured {}s)",
                    plan.recommended_timeout_secs, self.timeout_secs
                )
            })
    }

    /// Run one mutant and restore the source before returning.
    pub fn run_mutant(&self, mutant: &AstMutant, root: &Path) -> MutantExecutionResult {
        let start = Instant::now();
        let target_path = resolve_target_path(&mutant.file, root);
        let original_permissions = match ensure_regular_file(&target_path, root) {
            Ok(permissions) => permissions,
            Err(error) => {
                return mutant_error(
                    mutant,
                    self.test_cmd.as_deref().unwrap_or_default(),
                    start,
                    format!(
                        "Failed to inspect mutation target '{}': {error}",
                        target_path.display()
                    ),
                );
            }
        };
        let plan = self.resolve_test_plan(&mutant.file, root);
        if let Some(diagnostic) = self.full_suite_timeout_error(&plan) {
            let mut result = mutant_error(mutant, &plan.command, start, diagnostic);
            result.source_restored = true;
            return result;
        }
        let original_bytes = match fs::read(&target_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return mutant_error(
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
        let mut guard =
            RollbackGuard::new(&target_path, root, &original_bytes, original_permissions);
        let mut execution = apply_and_execute(
            self,
            MutationInput {
                mutant,
                target_path: &target_path,
                root,
                original_bytes: &original_bytes,
                plan: &plan,
            },
        );
        let restore_error = guard.restore().err();
        let source_restored = restore_error.is_none();
        if let Some(error) = restore_error {
            execution.outcome = MutantOutcome::RunnerError;
            append_diagnostic(
                &mut execution.diagnostic,
                format!(
                    "Failed to restore and verify original source '{}': {error}",
                    target_path.display(),
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
        if let Some(diagnostic) = self.full_suite_timeout_error(&plan) {
            return BaselineExecutionResult {
                file: file.to_path_buf(),
                outcome: BaselineOutcome::RunnerError,
                duration_ms: start.elapsed().as_millis(),
                command: plan.command,
                diagnostic,
            };
        }
        let target_path = resolve_target_path(file, root);
        let original = match snapshot_regular_file(&target_path, root) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return BaselineExecutionResult {
                    file: file.to_path_buf(),
                    outcome: BaselineOutcome::RunnerError,
                    duration_ms: start.elapsed().as_millis(),
                    command: plan.command,
                    diagnostic: format!(
                        "Failed to inspect baseline source '{}': {error}",
                        target_path.display()
                    ),
                };
            }
        };
        let execution =
            execute_with_timeout(&plan.command, process_roots(&plan), self.timeout_secs);
        let (outcome, diagnostic) = baseline_integrity(execution, &target_path, root, &original);
        BaselineExecutionResult {
            file: file.to_path_buf(),
            outcome,
            duration_ms: start.elapsed().as_millis(),
            command: plan.command,
            diagnostic: baseline_diagnostic(diagnostic, &plan.selection),
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

fn process_roots(plan: &ResolvedTestPlan) -> CommandRoots<'_> {
    CommandRoots {
        working_dir: &plan.working_dir,
        package_root: &plan.package_root,
        workspace_root: &plan.workspace_root,
    }
}

struct MutationInput<'a> {
    mutant: &'a AstMutant,
    target_path: &'a Path,
    root: &'a Path,
    original_bytes: &'a [u8],
    plan: &'a ResolvedTestPlan,
}

fn apply_and_execute(runner: &NativeMutationRunner, input: MutationInput<'_>) -> CommandExecution {
    match apply_mutant_bytes(
        input.target_path,
        input.root,
        input.mutant,
        input.original_bytes,
    ) {
        Ok(ApplyResult::Equivalent) => CommandExecution {
            outcome: MutantOutcome::Equivalent,
            diagnostic: "Replacement is byte-for-byte equivalent to the original source text."
                .to_string(),
            status: None,
        },
        Ok(ApplyResult::Applied) => execute_with_timeout(
            &input.plan.command,
            process_roots(input.plan),
            runner.timeout_secs,
        ),
        Err(error) => CommandExecution {
            outcome: if error.kind() == std::io::ErrorKind::InvalidInput {
                MutantOutcome::Unviable
            } else {
                MutantOutcome::RunnerError
            },
            diagnostic: format!("Failed to apply mutant {}: {error}", input.mutant.id),
            status: None,
        },
    }
}

fn mutant_error(
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

fn baseline_integrity(
    execution: CommandExecution,
    path: &Path,
    root: &Path,
    original: &SourceSnapshot,
) -> (BaselineOutcome, String) {
    let mut outcome = baseline_outcome(&execution);
    let mut diagnostic = execution.diagnostic;
    match verify_and_restore(path, root, original) {
        Ok(false) => {}
        Ok(true) => {
            outcome = BaselineOutcome::RunnerError;
            append_diagnostic(
                &mut diagnostic,
                format!(
                    "Baseline command modified source '{}'; original bytes and permissions were restored, so mutation testing was aborted.",
                    path.display()
                ),
            );
        }
        Err(error) => {
            outcome = BaselineOutcome::RunnerError;
            append_diagnostic(
                &mut diagnostic,
                format!(
                    "Baseline source integrity check failed for '{}': {error}; mutation testing was aborted.",
                    path.display()
                ),
            );
        }
    }
    (outcome, diagnostic)
}

fn baseline_diagnostic(mut diagnostic: String, selection: &TestSelection) -> String {
    if selection.is_full_suite() {
        let note = "full suite selected: no reliable relevant test was found";
        if diagnostic.is_empty() {
            return note.to_string();
        }
        diagnostic = append_output(diagnostic, note.to_string());
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
    root: &Path,
    mutant: &AstMutant,
    original_bytes: &[u8],
) -> std::io::Result<ApplyResult> {
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
    if mutant.replacement.as_bytes() == &original_bytes[mutant.start_byte..mutant.end_byte] {
        return Ok(ApplyResult::Equivalent);
    }
    let mut mutated = Vec::with_capacity(
        original_bytes.len() - (mutant.end_byte - mutant.start_byte) + mutant.replacement.len(),
    );
    mutated.extend_from_slice(&original_bytes[..mutant.start_byte]);
    mutated.extend_from_slice(mutant.replacement.as_bytes());
    mutated.extend_from_slice(&original_bytes[mutant.end_byte..]);
    let permissions = ensure_regular_file(target_path, root)?;
    atomic_replace(target_path, root, &mutated, &permissions).map(|()| ApplyResult::Applied)
}

#[derive(Clone, Copy)]
enum ApplyResult {
    Applied,
    Equivalent,
}

fn append_diagnostic(existing: &mut String, extra: String) {
    *existing = append_output(std::mem::take(existing), extra);
}
