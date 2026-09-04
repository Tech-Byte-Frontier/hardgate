use super::super::js::TestSelection;
use super::super::process::{CommandExecution, baseline_outcome, execute_with_timeout};
use super::plan::process_roots;
use super::restore::{SourceSnapshot, snapshot_protected_file, verify_and_restore};
use super::{BaselineExecutionResult, BaselineOutcome, NativeMutationRunner};
use crate::engines::process::append_output;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// One immutable snapshot set is shared by every distinct baseline command.
/// Keeping the set outside the command loop prevents a first command from
/// hiding changes made before a later command starts.
pub(crate) struct BaselineSources {
    entries: Vec<ProtectedSource>,
}

struct ProtectedSource {
    path: PathBuf,
    snapshot: SourceSnapshot,
}

pub(crate) fn snapshot_baseline_sources(
    files: &[PathBuf],
    root: &Path,
) -> io::Result<BaselineSources> {
    let mut entries = Vec::new();
    for file in files {
        let path = super::resolve_target_path(file, root);
        if entries
            .iter()
            .any(|entry: &ProtectedSource| entry.path == path)
        {
            continue;
        }
        let snapshot = snapshot_protected_file(&path, root)?;
        entries.push(ProtectedSource { path, snapshot });
    }
    if entries.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no protected production mutation sources were provided",
        ));
    }
    Ok(BaselineSources { entries })
}

impl NativeMutationRunner {
    /// Run the resolved test command against the unmodified source tree.
    pub fn run_baseline(&self, file: &Path, root: &Path) -> BaselineExecutionResult {
        let files = vec![file.to_path_buf()];
        let protected = match snapshot_baseline_sources(&files, root) {
            Ok(protected) => protected,
            Err(error) => {
                return BaselineExecutionResult {
                    file: file.to_path_buf(),
                    outcome: BaselineOutcome::RunnerError,
                    duration_ms: 0,
                    command: self.resolve_test_plan(file, root).command,
                    diagnostic: format!(
                        "Failed to snapshot protected baseline sources before execution: {error}"
                    ),
                };
            }
        };
        self.run_baseline_with_sources(file, root, &protected)
    }

    pub(crate) fn snapshot_baseline_sources(
        files: &[PathBuf],
        root: &Path,
    ) -> io::Result<BaselineSources> {
        snapshot_baseline_sources(files, root)
    }

    pub(crate) fn run_baseline_with_sources(
        &self,
        file: &Path,
        root: &Path,
        protected: &BaselineSources,
    ) -> BaselineExecutionResult {
        let start = Instant::now();
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            return BaselineExecutionResult {
                file: file.to_path_buf(),
                outcome: BaselineOutcome::RunnerError,
                duration_ms: start.elapsed().as_millis(),
                command: String::new(),
                diagnostic: super::unsupported_platform_diagnostic(),
            };
        }
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
        if let Some(diagnostic) = baseline_preflight(root, protected) {
            return BaselineExecutionResult {
                file: file.to_path_buf(),
                outcome: BaselineOutcome::RunnerError,
                duration_ms: start.elapsed().as_millis(),
                command: plan.command,
                diagnostic,
            };
        }
        let execution =
            execute_with_timeout(&plan.command, process_roots(&plan), self.timeout_secs);
        let (outcome, diagnostic) = baseline_integrity(execution, root, protected);
        BaselineExecutionResult {
            file: file.to_path_buf(),
            outcome,
            duration_ms: start.elapsed().as_millis(),
            command: plan.command,
            diagnostic: baseline_diagnostic(diagnostic, &plan.selection),
        }
    }
}

pub(crate) fn baseline_integrity(
    execution: CommandExecution,
    root: &Path,
    protected: &BaselineSources,
) -> (super::BaselineOutcome, String) {
    let mut outcome = baseline_outcome(&execution);
    let mut diagnostic = execution.diagnostic;
    for source in &protected.entries {
        match verify_and_restore(&source.path, root, &source.snapshot) {
            Ok(false) => {}
            Ok(true) => {
                outcome = super::BaselineOutcome::RunnerError;
                append_diagnostic(
                    &mut diagnostic,
                    format!(
                        "Baseline command modified source '{}' (protected target); original bytes, permissions, and file identity were restored, so mutation testing was aborted.",
                        source.path.display()
                    ),
                );
            }
            Err(error) => {
                outcome = super::BaselineOutcome::RunnerError;
                append_diagnostic(
                    &mut diagnostic,
                    format!(
                        "Baseline source integrity check failed for '{}': {error}; mutation testing was aborted.",
                        source.path.display()
                    ),
                );
            }
        }
    }
    (outcome, diagnostic)
}

/// Verify the immutable source set immediately before a command starts. This
/// closes the gap between one distinct baseline command and the next: an
/// external edit cannot be mistaken for a clean baseline or reach mutants.
pub(crate) fn baseline_preflight(root: &Path, protected: &BaselineSources) -> Option<String> {
    let mut diagnostic = String::new();
    for source in &protected.entries {
        match verify_and_restore(&source.path, root, &source.snapshot) {
            Ok(false) => {}
            Ok(true) => append_diagnostic(
                &mut diagnostic,
                format!(
                    "Protected source '{}' changed before baseline command; original bytes, permissions, and identity were restored. Mutation testing was aborted.",
                    source.path.display()
                ),
            ),
            Err(error) => append_diagnostic(
                &mut diagnostic,
                format!(
                    "Protected source '{}' could not be verified before baseline command: {error}; mutation testing was aborted.",
                    source.path.display()
                ),
            ),
        }
    }
    (!diagnostic.is_empty()).then_some(diagnostic)
}

pub(crate) fn baseline_diagnostic(mut diagnostic: String, selection: &TestSelection) -> String {
    if selection.is_full_suite() {
        let note = "full suite selected: no reliable relevant test was found";
        if diagnostic.is_empty() {
            return note.to_string();
        }
        diagnostic = append_output(diagnostic, note.to_string());
    }
    diagnostic
}

fn append_diagnostic(existing: &mut String, extra: String) {
    *existing = append_output(std::mem::take(existing), extra);
}
