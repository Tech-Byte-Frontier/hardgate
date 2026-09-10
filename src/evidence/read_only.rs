//! Project checks execute in a private copy; input writes cannot become fixes.
use super::{snapshot::Snapshot, workspace::EvidenceWorkspace};
use crate::engines::process::{ProcessOutcome, run_command_in_copy};
use anyhow::{Context, Result};
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
    failed: std::cell::Cell<bool>,
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
            failed: std::cell::Cell::new(false),
        })
    }

    pub(crate) fn run(&self, tokens: &[String], timeout: Duration) -> Result<ProcessOutcome> {
        let already_failed = self.failed.replace(true);
        let result = self.run_verified(tokens, timeout);
        if !matches!(&result, Ok(ProcessOutcome::Completed { status, .. }) if status.success()) {
            self.failed.set(true);
            self.workspace
                .failed("project check failed or restoration was incomplete")?;
        }
        self.workspace
            .diagnostics(&serde_json::to_string(tokens)?, &format!("{result:?}"))?;
        if matches!(&result, Ok(ProcessOutcome::Completed { status, .. }) if status.success()) {
            self.failed.set(already_failed);
        }
        let description = format!(
            "workspace lifecycle={} job={} workspace={} (preserved)",
            if crate::cancellation::signal().is_some() {
                "interrupted"
            } else {
                "failed"
            },
            self.workspace.job_path().display(),
            self.workspace.root().display()
        );
        result
            .map(|outcome| retained_outcome(outcome, &description))
            .with_context(|| description)
    }

    fn run_verified(&self, tokens: &[String], timeout: Duration) -> Result<ProcessOutcome> {
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
        if self.failed.get() {
            self.workspace.preserve()
        } else {
            self.before.require_same(
                &Snapshot::capture_with(&self.root, &self.input_policy)?,
                "checkout before check publication",
            )?;
            super::read_only_publication::publish(&self.workspace, &self.root)?;
            self.workspace.close()
        }
    }
}

fn retained_outcome(outcome: ProcessOutcome, description: &str) -> ProcessOutcome {
    match outcome {
        ProcessOutcome::Completed { status, mut output } if !status.success() => {
            output.push_str(&format!("\n{description}\n"));
            ProcessOutcome::Completed { status, output }
        }
        ProcessOutcome::TimedOut { mut output } => {
            output.push_str(&format!("\n{description}\n"));
            ProcessOutcome::TimedOut { output }
        }
        ProcessOutcome::Failed { message, output } => ProcessOutcome::Failed {
            message: format!("{message}; {description}"),
            output,
        },
        other => other,
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
