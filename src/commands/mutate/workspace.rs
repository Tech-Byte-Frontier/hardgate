//! Private source copy for CLI mutation, including dirty and untracked bytes.
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "workspace_copy.rs"]
mod copy;

static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);

pub(super) struct MutationWorkspace {
    root: PathBuf,
}

impl MutationWorkspace {
    pub(super) fn create(source: &Path, targets: &[PathBuf]) -> Result<Self> {
        crate::cancellation::install()?;
        let source = source.canonicalize()?;
        validate_targets(&source, targets)?;
        let workspace = Self {
            root: private_directory()?,
        };
        if workspace.root.starts_with(&source) {
            bail!(
                "mutation temporary directory must be outside the source workspace; set TMPDIR to an external directory"
            );
        }
        copy::copy_tree(&source, &workspace.root)?;
        for target in targets {
            if !workspace.root.join(target).is_file() {
                bail!(
                    "mutation target `{}` is outside the supported source snapshot",
                    target.display()
                );
            }
        }
        Ok(workspace)
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn close(self) -> Result<()> {
        fs::remove_dir_all(&self.root).with_context(|| {
            format!(
                "failed to remove mutation workspace `{}`",
                self.root.display()
            )
        })
    }
}

impl Drop for MutationWorkspace {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "hardgate: retained mutation workspace {}: {error}",
                self.root.display()
            );
        }
    }
}

fn private_directory() -> Result<PathBuf> {
    for _ in 0..100 {
        let id = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("hardgate-mutation-{}-{id}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&root) {
            Ok(()) => return Ok(root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    bail!("unable to allocate a private mutation workspace")
}

fn validate_targets(root: &Path, targets: &[PathBuf]) -> Result<()> {
    for target in targets {
        let path = root.join(target);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if fs::metadata(&path)?.nlink() > 1 {
                bail!(
                    "refusing mutation target `{}`: source has pre-existing hardlinks",
                    target.display()
                );
            }
        }
        if !path.canonicalize()?.starts_with(root) {
            bail!(
                "mutation target escapes the source workspace: {}",
                target.display()
            );
        }
    }
    Ok(())
}
