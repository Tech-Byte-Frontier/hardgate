//! Project checks execute in a private copy; input writes cannot become fixes.
use super::{snapshot::Snapshot, workspace::EvidenceWorkspace};
use crate::engines::process::{ProcessOutcome, run_command_in_copy};
use anyhow::{Result, ensure};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) fn run(tokens: &[String], root: &Path, timeout: Duration) -> ProcessOutcome {
    match execute(tokens, root, timeout) {
        Ok(outcome) => outcome,
        Err(error) => ProcessOutcome::Failed {
            message: format!("read-only project check failed: {error:#}"),
            output: String::new(),
        },
    }
}

fn execute(tokens: &[String], root: &Path, timeout: Duration) -> Result<ProcessOutcome> {
    let session = Session::create(root)?;
    let outcome = session.run(tokens, timeout);
    session.close()?;
    outcome
}

pub(crate) struct Session {
    root: PathBuf,
    before: Snapshot,
    workspace: EvidenceWorkspace,
}

impl Session {
    pub(crate) fn create(root: &Path) -> Result<Self> {
        ensure!(
            crate::resources::runtime::inherited()?,
            "external checks require verified CPU and memory containment"
        );
        let root = root.canonicalize()?;
        let before = Snapshot::capture(&root)?;
        let workspace = EvidenceWorkspace::create(&root, &[])?;
        before.require_same(
            &Snapshot::capture(workspace.root())?,
            "read-only check copy",
        )?;
        Ok(Self {
            root,
            before,
            workspace,
        })
    }

    pub(crate) fn run(&self, tokens: &[String], timeout: Duration) -> Result<ProcessOutcome> {
        self.before.require_same(
            &Snapshot::capture(self.workspace.root())?,
            "check inputs before command",
        )?;
        let outcome = run_command_in_copy(
            tokens,
            (self.workspace.root(), &self.root),
            timeout,
            "evidence",
        );
        let copy_state = self.before.require_same(
            &Snapshot::capture(self.workspace.root())?,
            "check command wrote project inputs",
        );
        let source_state = self.before.require_same(
            &Snapshot::capture(&self.root)?,
            "checkout changed during check",
        );
        copy_state?;
        source_state?;
        Ok(remap_output(
            outcome,
            &self.workspace.root().display().to_string(),
            &self.root.display().to_string(),
        ))
    }

    pub(crate) fn close(self) -> Result<()> {
        self.workspace.close()
    }
}

fn remap_output(outcome: ProcessOutcome, copied: &str, source: &str) -> ProcessOutcome {
    match outcome {
        ProcessOutcome::Completed { status, output } => ProcessOutcome::Completed {
            status,
            output: output.replace(copied, source),
        },
        ProcessOutcome::TimedOut { output } => ProcessOutcome::TimedOut {
            output: output.replace(copied, source),
        },
        ProcessOutcome::Failed { message, output } => ProcessOutcome::Failed {
            message: message.replace(copied, source),
            output: output.replace(copied, source),
        },
    }
}
