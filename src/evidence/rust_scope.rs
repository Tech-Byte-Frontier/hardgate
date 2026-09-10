//! Narrow filters are allowed for samples, never an exhaustive contract.
use anyhow::{Result, ensure};
use std::path::Path;

pub(super) fn validate(
    options: &super::EvidenceOptions,
    root: &Path,
    partition: &super::partitions::Partition,
) -> Result<()> {
    if partition.config.scope == crate::config::MutationScope::Sample {
        return Ok(());
    }
    for arg in &options.args {
        ensure!(
            !matches!(arg.split('=').next(), Some("--re" | "--file" | "--shard")),
            "exhaustive cargo-mutants partitions cannot use mutant filters or shards; declare scope='sample' for sampled diagnostics"
        );
    }
    let path = root.join(".cargo/mutants.toml");
    if path.exists() {
        let config: toml::Table = toml::from_str(&std::fs::read_to_string(path)?)?;
        // Unknown settings need an explicit adapter review before they can
        // silently narrow generated mutants in an exhaustive partition.
        let supported = [
            "test_tool",
            "additional_cargo_args",
            "additional_cargo_test_args",
            "all_features",
            "no_default_features",
            "features",
            "timeout",
            "timeout_multiplier",
            "build_timeout",
            "build_timeout_multiplier",
            "minimum_test_timeout",
            "output",
            "jobs",
        ];
        for key in config.keys() {
            ensure!(
                supported.contains(&key.as_str()),
                "cargo-mutants setting `{key}` is not validated for exhaustive evidence; use a sample or remove the unsupported selector"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "rust_scope_tests.rs"]
mod tests;
