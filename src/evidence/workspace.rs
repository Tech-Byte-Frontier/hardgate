//! Private producer workspace, including dirty and untracked project inputs.
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "workspace_copy.rs"]
mod copy;

static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);

pub(super) struct EvidenceWorkspace {
    root: PathBuf,
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
        let source = source.canonicalize()?;
        let workspace = Self {
            root: private_directory()?,
        };
        if workspace.root.starts_with(&source) {
            bail!(
                "evidence temporary directory must be outside the source workspace; set TMPDIR to an external directory"
            );
        }
        copy::copy_tree(&source, &workspace.root)?;
        Ok(workspace)
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn close(self) -> Result<()> {
        fs::remove_dir_all(&self.root).with_context(|| {
            format!(
                "failed to remove evidence workspace `{}`",
                self.root.display()
            )
        })
    }
}

impl Drop for EvidenceWorkspace {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "hardgate: retained evidence workspace {}: {error}",
                self.root.display()
            );
        }
    }
}

fn private_directory() -> Result<PathBuf> {
    for _ in 0..100 {
        let id = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("hardgate-evidence-{}-{id}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&root) {
            Ok(()) => return Ok(root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).with_context(|| format!(
                "could not create private workspace under selected TMPDIR={}; this failure precedes Hardgate containment. Check host/agent permissions or set TMPDIR to a writable directory outside the project", std::env::temp_dir().display()
            )),
        }
    }
    bail!("unable to allocate a private evidence workspace")
}
