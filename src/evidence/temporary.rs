use crate::engines::process::ProcessOutcome;
use std::path::{Path, PathBuf};

pub(crate) fn scratch_directory(workspace: &Path) -> PathBuf {
    workspace.join(".hardgate/evidence/tmp")
}

pub(super) fn explain(outcome: ProcessOutcome, workspace: &Path) -> ProcessOutcome {
    match outcome {
        ProcessOutcome::Completed { status, output } if !status.success() => {
            ProcessOutcome::Completed {
                status,
                output: annotate(output, workspace),
            }
        }
        ProcessOutcome::Failed { message, output } => ProcessOutcome::Failed {
            message,
            output: annotate(output, workspace),
        },
        other => other,
    }
}

fn annotate(mut output: String, workspace: &Path) -> String {
    if crate::resources::runtime::isolated()
        && output.contains("/tmp/")
        && (output.contains("Permission denied") || output.contains("Operation not permitted"))
    {
        output.push_str(&format!(
            "\nHardgate containment restricts writes to the disposable workspace. Runtime writable TMPDIR={} (removed after this check). Use mktemp -d \"${{TMPDIR:-/tmp}}/finance-hardgate.XXXXXXXX\" inside the configured command. A shell outside Hardgate must use its own host/agent TMPDIR.\n",
            scratch_directory(workspace).display()
        ));
    }
    output
}
