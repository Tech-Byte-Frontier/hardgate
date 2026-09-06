use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Project inputs, independent of timestamps and the checkout's absolute path.
/// Installed dependencies and compiler/VCS data are identified by their project
/// manifests/lockfiles, not presented as a hermetic build environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot(pub BTreeMap<PathBuf, String>);

pub(super) const OUTPUT_DIRECTORY: &str = ".hardgate/evidence";

pub(super) fn omitted(path: &Path, dependencies: bool) -> bool {
    path.starts_with(OUTPUT_DIRECTORY)
        || path.components().any(|part| {
            matches!(part.as_os_str().to_str(), Some(".git" | "target"))
                || (dependencies && part.as_os_str() == "node_modules")
        })
}

impl Snapshot {
    pub(super) fn capture(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let mut files = BTreeMap::new();
        let mut pending = vec![PathBuf::new()];
        while let Some(relative) = pending.pop() {
            for entry in fs::read_dir(root.join(&relative))? {
                crate::cancellation::check()?;
                crate::resources::check_pressure()?;
                let entry = entry?;
                let path = relative.join(entry.file_name());
                if omitted(&path, true) {
                    continue;
                }
                let metadata = fs::symlink_metadata(entry.path())?;
                if metadata.is_symlink() {
                    let target = entry.path().canonicalize()?;
                    let target = target.strip_prefix(&root).with_context(|| {
                        format!("source symlink leaves the workspace: {}", path.display())
                    })?;
                    if omitted(target, true) {
                        bail!(
                            "source symlink targets unbound build/dependency data: {}",
                            path.display()
                        );
                    }
                    files.insert(path, format!("symlink:{}", target.display()));
                } else if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file() {
                    files.insert(path, file_hash(&entry.path())?);
                } else {
                    bail!(
                        "unsupported special file in evidence inputs: {}",
                        path.display()
                    );
                }
            }
        }
        Ok(Self(files))
    }

    pub(super) fn require_same(&self, other: &Self, stage: &str) -> Result<()> {
        if self == other {
            return Ok(());
        }
        let changed = self
            .0
            .keys()
            .chain(other.0.keys())
            .find(|path| self.0.get(*path) != other.0.get(*path));
        bail!(
            "{stage}: source/test/config inputs changed{}; regenerate evidence from the current inputs",
            changed
                .map(|path| format!(" at {}", path.display()))
                .unwrap_or_default()
        );
    }
}

pub(super) fn file_hash(path: &Path) -> Result<String> {
    let mut input = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        crate::cancellation::check()?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
