//! Private producer workspace, including dirty and untracked project inputs.
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "workspace_copy.rs"]
mod copy;
#[path = "workspace_lifecycle.rs"]
mod lifecycle;

static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);

pub(super) struct EvidenceWorkspace {
    root: PathBuf,
    directory: Option<fs::File>,
    job: lifecycle::ManagedJob,
    closed: bool,
}

impl EvidenceWorkspace {
    pub(super) fn create_verified(
        root: &Path,
        policy: &super::inputs::InputPolicy,
        before: &super::Snapshot,
    ) -> Result<Self> {
        let workspace = Self::create(root)?;
        before.require_same(
            &super::Snapshot::capture_with(workspace.root(), policy)?,
            "producer copy",
        )?;
        before.require_same(
            &super::Snapshot::capture_with(root, policy)?,
            "checkout during copy",
        )?;
        Ok(workspace)
    }

    pub(super) fn create(source: &Path) -> Result<Self> {
        crate::cancellation::install()?;
        Self::create_at(source, &super::temporary::managed_root()?)
    }

    pub(super) fn create_at(source: &Path, scratch: &Path) -> Result<Self> {
        let source = source.canonicalize()?;
        let scratch = resolved_destination(scratch)?;
        if scratch.starts_with(&source) {
            bail!("evidence temporary directory must be outside the source workspace");
        }
        fs::create_dir_all(&scratch)?;
        let scratch = scratch.canonicalize()?;
        if scratch.starts_with(&source) {
            bail!(
                "evidence temporary directory must be outside the source workspace; set TMPDIR or HARDGATE_SCRATCH_ROOT to an external directory"
            );
        }
        let job = lifecycle::ManagedJob::create(private_directory(&scratch)?, &source)?;
        let root = job.root.join("work");
        let mut workspace = Self {
            root,
            directory: None,
            job,
            closed: false,
        };
        fs::create_dir(&workspace.root)?;
        workspace.directory = Some(fs::File::open(&workspace.root)?);
        crate::engines::process::diagnostic(format_args!(
            "hardgate: workspace lifecycle=active job={} workspace={}",
            workspace.job.root.display(),
            workspace.root.display()
        ));
        copy::copy_tree(&source, &workspace.root)?;
        workspace
            .job
            .status("active", "isolated inputs copied; executing checks")?;
        Ok(workspace)
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }
    pub(super) fn job_path(&self) -> &Path {
        &self.job.root
    }

    pub(super) fn diagnostics(&self, stage: &str, output: &str) -> Result<()> {
        self.job.diagnostics(stage, output)
    }

    pub(super) fn failed(&self, stage: &str) -> Result<()> {
        let state = if crate::cancellation::signal().is_some() {
            "interrupted"
        } else if self.job.current_status() == "publishing" {
            "publication-failed"
        } else {
            "failed"
        };
        self.job.status(state, stage)
    }

    pub(super) fn begin_publication(&self) -> Result<()> {
        self.job.status(
            "publishing",
            "saving and verifying reports/receipts outside disposable workspace",
        )
    }

    pub(super) fn preserve(mut self) -> Result<()> {
        if matches!(self.job.current_status().as_str(), "active" | "publishing") {
            self.failed("operation did not complete")?;
        }
        self.job.report();
        self.closed = true;
        Ok(())
    }

    pub(super) fn close(mut self) -> Result<()> {
        crate::cancellation::check()?;
        anyhow::ensure!(
            matches!(self.job.current_status().as_str(), "active" | "publishing"),
            "failed workspaces must be retained for diagnosis"
        );
        self.job.verify_owner()?;
        self.verify_directory()?;
        self.job.completed()?;
        let cleanup =
            fs::remove_dir_all(&self.root).and_then(|()| fs::remove_dir_all(&self.job.root));
        if let Err(error) = cleanup {
            let _ = fs::remove_file(self.job.root.join(".completed"));
            let detail = format!("failed to remove disposable workspace: {error}");
            if let Err(state_error) = self.job.status("cleanup-failed", &detail) {
                crate::engines::process::diagnostic(format_args!(
                    "hardgate: could not save workspace cleanup failure: {state_error:#}"
                ));
            }
            return Err(error).with_context(|| {
                format!(
                    "workspace lifecycle=cleanup-failed job={} workspace={} (preserved)",
                    self.job.root.display(),
                    self.root.display()
                )
            });
        }
        self.closed = true;
        crate::engines::process::diagnostic(format_args!(
            "hardgate: workspace lifecycle=completed job={} (removed)",
            self.job.root.display()
        ));
        Ok(())
    }

    fn verify_directory(&self) -> Result<()> {
        let current = fs::symlink_metadata(&self.root)?;
        anyhow::ensure!(
            current.is_dir() && !current.is_symlink(),
            "workspace directory was replaced; refusing cleanup"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = self
                .directory
                .as_ref()
                .context("workspace directory was not opened")?
                .metadata()?;
            anyhow::ensure!(
                current.ino() == held.ino() && current.dev() == held.dev(),
                "workspace directory identity changed; refusing cleanup"
            );
        }
        Ok(())
    }
}

/// Resolve existing aliases and normalize the missing suffix before any mkdir.
pub(super) fn resolved_destination(path: &Path) -> Result<PathBuf> {
    use std::path::Component;
    let mut resolved = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => {
                resolved.push(other.as_os_str());
                resolved = resolve_existing(resolved)?;
            }
        }
    }
    Ok(resolved)
}

impl Drop for EvidenceWorkspace {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        if matches!(self.job.current_status().as_str(), "active" | "publishing")
            && let Err(error) = self.failed("operation returned without verified completion")
        {
            crate::engines::process::diagnostic(format_args!(
                "hardgate: could not save retained workspace status: {error:#}"
            ));
        }
        self.job.report();
    }
}

fn private_directory(scratch: &Path) -> Result<PathBuf> {
    for _ in 0..100 {
        let id = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = scratch.join(format!("job-hardgate-{}-{nonce}-{id}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&root) {
            Ok(()) => return Ok(root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).with_context(|| format!("could not create managed workspace under scratch root {}; check host/agent permissions or set TMPDIR/HARDGATE_SCRATCH_ROOT to a writable directory outside the project", scratch.display())),
        }
    }
    bail!("unable to allocate a managed evidence workspace")
}

#[cfg(test)]
#[path = "workspace_lifecycle_tests.rs"]
mod tests;

fn resolve_existing(path: PathBuf) -> Result<PathBuf> {
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(path.canonicalize()?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(error.into()),
    }
}
