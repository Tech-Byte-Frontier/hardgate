use super::generator::AstMutant;
use super::js::{ResolvedTestPlan, TestSelection, is_javascript_path, resolve_js_test_plan};
use super::process::CommandExecution;
use crate::engines::process::append_output;
#[path = "apply.rs"]
mod apply;
#[path = "baseline.rs"]
mod baseline;
pub(crate) use baseline::BaselineRunContext;
#[path = "plan.rs"]
mod plan;
#[path = "restore.rs"]
mod restore;
use anyhow::Result;
use apply::{MutationInput, apply_and_execute};
use plan::{custom_plan, plain_plan, rust_plan};
use restore::{
    RestoreLocation, SourceSnapshot, has_multiple_links, open_location, restore_mutation_location,
    snapshot_location, verify_live_path, verify_unchanged,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Maximum timeout used when a JavaScript project has no trustworthy file
/// mapping and the complete test suite must run.
pub const FULL_SUITE_TIMEOUT_SECS: u64 = 60;
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug)]
pub(crate) enum MutationRunnerError {
    Resolution(String),
    Integrity(String),
}

impl MutationRunnerError {
    pub(crate) fn resolution(error: impl std::fmt::Display) -> Self {
        Self::Resolution(error.to_string())
    }

    pub(crate) fn integrity(message: impl Into<String>) -> Self {
        Self::Integrity(message.into())
    }

    fn source_intact(&self) -> bool {
        matches!(self, Self::Resolution(_))
    }
}

impl std::fmt::Display for MutationRunnerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resolution(message) | Self::Integrity(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for MutationRunnerError {}

pub(crate) type MutationRunnerResult<T> = std::result::Result<T, MutationRunnerError>;

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
    location: &'a RestoreLocation,
    original: &'a SourceSnapshot,
    expected_mutation: Option<&'a SourceSnapshot>,
    restored: bool,
}

impl<'a> RollbackGuard<'a> {
    fn new(
        file_path: &'a Path,
        location: &'a RestoreLocation,
        original: &'a SourceSnapshot,
    ) -> Self {
        Self {
            file_path,
            location,
            original,
            expected_mutation: None,
            restored: false,
        }
    }

    fn expect_mutation(&mut self, snapshot: &'a SourceSnapshot) {
        self.expected_mutation = Some(snapshot);
    }

    fn restore(&mut self) -> std::io::Result<()> {
        let result = match self.expected_mutation {
            Some(expected) => restore_mutation_location(self.location, self.original, expected),
            None => verify_unchanged(self.location, self.original),
        };
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
        match self.restore() {
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
    pub fn resolve_test_plan(&self, file: &Path, root: &Path) -> Result<ResolvedTestPlan> {
        if let Some(command) = self.test_cmd.as_deref() {
            return Ok(custom_plan(command, file, root));
        }
        if is_javascript_path(file) {
            return resolve_js_test_plan(file, root).map_err(|error| {
                anyhow::anyhow!(
                    "failed to resolve JavaScript mutation test plan for `{}`: {error:#}",
                    file.display()
                )
            });
        }
        if file.extension().and_then(|value| value.to_str()) == Some("rs") {
            Ok(rust_plan(file, root))
        } else {
            Ok(plain_plan(
                "cargo test".to_string(),
                root,
                TestSelection::Custom,
            ))
        }
    }

    pub fn resolve_test_command(&self, file: &Path, root: &Path) -> Result<String> {
        Ok(self.resolve_test_plan(file, root)?.command)
    }

    /// Pick a safe default timeout when an automatically resolved JS command
    /// has to execute an entire suite. Explicit user/configured values win.
    pub fn default_timeout_secs(
        files: &[PathBuf],
        root: &Path,
        test_cmd: Option<&str>,
    ) -> Result<u64> {
        if test_cmd.is_none() {
            let runner = Self::new(DEFAULT_TIMEOUT_SECS, None);
            for file in files {
                if is_javascript_path(file)
                    && runner
                        .resolve_test_plan(file, root)?
                        .selection
                        .is_full_suite()
                {
                    return Ok(FULL_SUITE_TIMEOUT_SECS);
                }
            }
        }
        Ok(DEFAULT_TIMEOUT_SECS)
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
        match self.try_run_mutant(mutant, root) {
            Ok(result) => result,
            Err(error) => {
                let source_restored = error.source_intact();
                let mut result = mutant_error(
                    mutant,
                    self.test_cmd.as_deref().unwrap_or_default(),
                    start,
                    error.to_string(),
                );
                result.source_restored = source_restored;
                result
            }
        }
    }

    pub(crate) fn try_run_mutant(
        &self,
        mutant: &AstMutant,
        root: &Path,
    ) -> MutationRunnerResult<MutantExecutionResult> {
        let start = Instant::now();
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            return Ok(mutant_error(
                mutant,
                self.test_cmd.as_deref().unwrap_or_default(),
                start,
                unsupported_platform_diagnostic(),
            ));
        }
        let prepared = match prepare_target(mutant, root) {
            Ok(prepared) => prepared,
            Err(error) => {
                return Ok(mutant_error(
                    mutant,
                    self.test_cmd.as_deref().unwrap_or_default(),
                    start,
                    error,
                ));
            }
        };
        let PreparedTarget {
            target_path,
            location,
            original,
        } = prepared;
        if has_multiple_links(&original) {
            let mut result = mutant_error(
                mutant,
                self.test_cmd.as_deref().unwrap_or_default(),
                start,
                format!(
                    "refusing mutation target '{}': source has pre-existing hardlinks; no command was executed",
                    target_path.display()
                ),
            );
            verify_no_write(&mut result, &location, &original, &target_path);
            return Ok(result);
        }
        let plan = self
            .resolve_test_plan(&mutant.file, root)
            .map_err(|error| {
                resolution_failure_after_prepare(error, &location, &original, &target_path)
            })?;
        if let Some(diagnostic) = self.full_suite_timeout_error(&plan) {
            let mut result = mutant_error(mutant, &plan.command, start, diagnostic);
            verify_no_write(&mut result, &location, &original, &target_path);
            return Ok(result);
        }
        let (execution, source_restored) = execute_and_restore(MutationContext {
            runner: self,
            mutant,
            target_path: &target_path,
            location: &location,
            original: &original,
            plan: &plan,
        });
        Ok(MutantExecutionResult {
            mutant: mutant.clone(),
            outcome: execution.outcome,
            duration_ms: start.elapsed().as_millis(),
            command: plan.command,
            diagnostic: execution.diagnostic,
            source_restored,
        })
    }
}

fn resolution_failure_after_prepare(
    error: anyhow::Error,
    location: &RestoreLocation,
    original: &SourceSnapshot,
    target_path: &Path,
) -> MutationRunnerError {
    match verify_unchanged(location, original) {
        Ok(()) => MutationRunnerError::resolution(error),
        Err(integrity_error) => MutationRunnerError::integrity(format!(
            "{error:#}; failed to verify unchanged source `{}` after test resolution failed: {integrity_error}",
            target_path.display()
        )),
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

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported_platform_diagnostic() -> String {
    "mutation runner requires Linux/macOS process-group cleanup and descriptor-relative atomic source replacement; this platform is unsupported and no baseline or source write was attempted".to_string()
}

struct PreparedTarget {
    target_path: PathBuf,
    location: RestoreLocation,
    original: SourceSnapshot,
}

struct MutationContext<'a> {
    runner: &'a NativeMutationRunner,
    mutant: &'a AstMutant,
    target_path: &'a Path,
    location: &'a RestoreLocation,
    original: &'a SourceSnapshot,
    plan: &'a ResolvedTestPlan,
}

fn execute_and_restore(context: MutationContext<'_>) -> (CommandExecution, bool) {
    let mut expected_mutation = None;
    let mut guard = RollbackGuard::new(context.target_path, context.location, context.original);
    let mut execution = apply_and_execute(
        context.runner,
        MutationInput {
            mutant: context.mutant,
            location: context.location,
            original: context.original,
            plan: context.plan,
        },
        &mut expected_mutation,
    );
    if let Some(snapshot) = expected_mutation.as_ref() {
        guard.expect_mutation(snapshot);
    }
    let restore_error = guard.restore().err();
    let source_restored = restore_error.is_none();
    if let Some(error) = restore_error {
        execution.outcome = MutantOutcome::RunnerError;
        append_diagnostic(
            &mut execution.diagnostic,
            format!(
                "Failed to restore and verify original source '{}': {error}",
                context.target_path.display(),
            ),
        );
    }
    drop(guard);
    (execution, source_restored)
}

fn prepare_target(mutant: &AstMutant, root: &Path) -> Result<PreparedTarget, String> {
    let target_path = resolve_target_path(&mutant.file, root);
    let location = open_location(&target_path, root).map_err(|error| {
        format!(
            "Failed to inspect mutation target '{}': {error}",
            target_path.display()
        )
    })?;
    verify_live_path(&location, &target_path, root).map_err(|error| {
        format!(
            "Failed to verify live mutation target '{}': {error}",
            target_path.display()
        )
    })?;
    let original = match snapshot_location(&location) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            return Err(format!(
                "Failed to inspect mutation target '{}': target does not exist",
                target_path.display()
            ));
        }
        Err(error) => {
            return Err(format!(
                "Failed to inspect mutation target '{}': {error}",
                target_path.display()
            ));
        }
    };
    Ok(PreparedTarget {
        target_path,
        location,
        original,
    })
}

fn resolve_target_path(file: &Path, root: &Path) -> PathBuf {
    if file.is_absolute() {
        file.to_path_buf()
    } else {
        std::fs::canonicalize(root)
            .unwrap_or_else(|_| root.to_path_buf())
            .join(file)
    }
}

fn append_diagnostic(existing: &mut String, extra: String) {
    *existing = append_output(std::mem::take(existing), extra);
}

fn verify_no_write(
    result: &mut MutantExecutionResult,
    location: &RestoreLocation,
    original: &SourceSnapshot,
    target_path: &Path,
) {
    match verify_unchanged(location, original) {
        Ok(()) => result.source_restored = true,
        Err(error) => append_diagnostic(
            &mut result.diagnostic,
            format!(
                "Failed to verify unchanged source '{}': {error}",
                target_path.display()
            ),
        ),
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "runner_tests.rs"]
mod tests;
