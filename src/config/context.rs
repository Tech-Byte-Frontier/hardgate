use super::HardgateConfig;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// One invocation's policy authority. No process cwd or environment is changed.
#[derive(Debug, Clone)]
pub struct ConfigContext {
    pub invocation_dir: PathBuf,
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub config: HardgateConfig,
}

impl ConfigContext {
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        Self::load_from(&std::env::current_dir()?, explicit)
    }

    pub fn load_from(invocation: &Path, explicit: Option<&Path>) -> Result<Self> {
        let invocation_dir = invocation
            .canonicalize()
            .context("Cannot resolve invocation directory")?;
        let (root, config_path) = locate(&invocation_dir, explicit)?;
        let config = HardgateConfig::load_resolved(config_path.as_deref())?;
        Ok(Self {
            invocation_dir,
            root,
            config_path,
            config,
        })
    }

    /// User-supplied paths retain their meaning relative to the invocation.
    pub fn input_path(&self, path: &Path) -> PathBuf {
        self.invocation_dir.join(path)
    }

    pub fn input_paths(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        paths.iter().map(|path| self.input_path(path)).collect()
    }

    pub fn resolve_gate_paths(&self, paths: &mut Vec<PathBuf>, coverage: &mut Option<String>) {
        *paths = self.input_paths(paths);
        *coverage = self.input_report(coverage.take());
    }

    /// Resolve an optional CLI evidence report without requiring it to exist yet.
    pub fn input_report(&self, path: Option<String>) -> Option<String> {
        path.map(|path| {
            self.input_path(Path::new(&path))
                .to_string_lossy()
                .into_owned()
        })
    }

    /// Policy paths, including evidence reports, belong to the policy root.
    pub fn policy_path(&self, path: &Path) -> PathBuf {
        self.root.join(path)
    }
}

/// Search the nearest policy first, stopping at the first Git boundary (including
/// worktree `.git` files). Without a policy, use that Git root or the invocation.
fn locate(invocation: &Path, explicit: Option<&Path>) -> Result<(PathBuf, Option<PathBuf>)> {
    if let Some(explicit) = explicit {
        let path = invocation.join(explicit).canonicalize().with_context(|| {
            format!(
                "Explicit config path does not exist or is unreadable: {}",
                explicit.display()
            )
        })?;
        if !path.is_file() {
            bail!("Explicit config path is not a file: {}", path.display());
        }
        return Ok((
            path.parent()
                .context("Config has no parent directory")?
                .to_path_buf(),
            Some(path),
        ));
    }
    for directory in invocation.ancestors() {
        let path = directory.join("hardgate.toml");
        if entry_exists(&path)? {
            // Preserve the discovered directory as authority even for a symlinked policy.
            return Ok((directory.to_path_buf(), Some(path)));
        }
        if entry_exists(&directory.join(".git"))? {
            return Ok((directory.to_path_buf(), None));
        }
    }
    Ok((invocation.to_path_buf(), None))
}

fn entry_exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("Cannot inspect policy boundary: {}", path.display())),
    }
}
