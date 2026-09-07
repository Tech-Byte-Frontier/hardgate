//! Project checks execute in a private copy; input writes cannot become fixes.
use super::{snapshot::Snapshot, workspace::EvidenceWorkspace};
use crate::engines::process::{ProcessOutcome, run_command_in_copy};
use anyhow::Result;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) fn run(
    tokens: &[String],
    root: &Path,
    timeout: Duration,
    require_isolation: bool,
) -> ProcessOutcome {
    match execute(tokens, root, timeout, require_isolation) {
        Ok(outcome) => outcome,
        Err(error) => ProcessOutcome::Failed {
            message: format!("read-only project check failed: {error:#}"),
            output: String::new(),
        },
    }
}

fn execute(
    tokens: &[String],
    root: &Path,
    timeout: Duration,
    require_isolation: bool,
) -> Result<ProcessOutcome> {
    let mut config = crate::config::HardgateConfig::default();
    config.orchestration.require_isolation = require_isolation;
    let session = Session::create_for(root, &config)?;
    let outcome = session.run(tokens, timeout);
    session.close()?;
    outcome
}

pub(crate) struct Session {
    root: PathBuf,
    before: Snapshot,
    workspace: EvidenceWorkspace,
    input_policy: super::inputs::InputPolicy,
    isolated: bool,
}

impl Session {
    pub(crate) fn create_for(root: &Path, config: &crate::config::HardgateConfig) -> Result<Self> {
        if config.orchestration.require_isolation {
            crate::resources::runtime::require()?;
        }
        let root = root.canonicalize()?;
        let input_policy = super::inputs::InputPolicy::new(&root, config)?;
        let before = Snapshot::capture_with(&root, &input_policy)?;
        let workspace = EvidenceWorkspace::create(&root)?;
        before.require_same(
            &Snapshot::capture_with(workspace.root(), &input_policy)?,
            "read-only check copy",
        )?;
        Ok(Self {
            root,
            before,
            workspace,
            input_policy,
            isolated: config.orchestration.require_isolation
                || crate::resources::runtime::isolated(),
        })
    }

    pub(crate) fn run(&self, tokens: &[String], timeout: Duration) -> Result<ProcessOutcome> {
        self.before.require_same(
            &Snapshot::capture_with(self.workspace.root(), &self.input_policy)?,
            "check inputs before command",
        )?;
        let outcome = run_command_in_copy(
            tokens,
            (self.workspace.root(), &self.root),
            timeout,
            if self.isolated { "evidence" } else { "check" },
        );
        let copy_state = self.before.require_same(
            &Snapshot::capture_with(self.workspace.root(), &self.input_policy)?,
            "check command wrote project inputs",
        );
        let source_state = self.before.require_same(
            &Snapshot::capture_with(&self.root, &self.input_policy)?,
            "checkout changed during check",
        );
        copy_state?;
        source_state?;
        Ok(super::temporary::explain(
            remap_output(
                outcome,
                &self.workspace.root().display().to_string(),
                &self.root.display().to_string(),
            ),
            self.workspace.root(),
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
