use crate::evidence::Producer;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Explicit producer partitions, independent from acceptance thresholds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceConfig {
    #[serde(default)]
    pub producers: BTreeMap<String, ProducerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerConfig {
    pub producer: Producer,
    /// Repository-local tool configuration, included in the input binding.
    pub config: Option<PathBuf>,
    /// Positive source globs forming this producer's expected partition.
    pub sources: Vec<String>,
    #[serde(default)]
    pub scope: MutationScope,
    pub toolchain: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MutationScope {
    #[default]
    Exhaustive,
    Sample,
}

fn default_timeout() -> u64 {
    1200
}

impl EvidenceConfig {
    pub(super) fn validate(&self) -> Result<()> {
        for (name, producer) in &self.producers {
            ensure!(
                valid_name(name),
                "invalid evidence producer name `{name}`: use letters, digits, '-' or '_'"
            );
            ensure!(
                producer.timeout_secs > 0,
                "evidence producer `{name}` timeout must be positive"
            );
            ensure!(
                !producer.sources.is_empty(),
                "evidence producer `{name}` requires explicit source globs"
            );
            for pattern in &producer.sources {
                ensure!(
                    !pattern.starts_with('!') && safe_relative(Path::new(pattern)),
                    "evidence source glob must be positive and repository-relative: {pattern}"
                );
                globset::Glob::new(pattern)?;
            }
            if let Some(path) = &producer.config {
                ensure!(
                    safe_relative(path),
                    "producer config must be repository-relative without traversal"
                );
            }
        }
        Ok(())
    }
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(crate) fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
