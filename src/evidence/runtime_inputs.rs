//! Conservative local reuse identity. Values are hashed together so receipts
//! never disclose environment secrets. This is not a hermetic build claim.
use super::{Producer, snapshot::file_hash};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeInputs {
    hardgate_sha256: String,
    environment_sha256: String,
    installed_inputs: BTreeMap<PathBuf, String>,
}

static EXECUTABLE_IDENTITY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn hardgate_identity() -> Result<String> {
    if let Some(identity) = EXECUTABLE_IDENTITY.get() {
        return Ok(identity.clone());
    }
    let identity = file_hash(&std::env::current_exe()?)?;
    let _ = EXECUTABLE_IDENTITY.set(identity.clone());
    Ok(identity)
}

/// Overrides may load code or configuration outside the bound installation.
/// Keep these executions cold until a producer can bind those extra inputs.
pub(super) fn can_reuse(root: &Path, producer: Producer) -> bool {
    if producer == Producer::Pytest
        && ![".venv/bin/python", "venv/bin/python"]
            .iter()
            .any(|path| root.join(path).is_file())
    {
        return false;
    }
    !external_overrides(producer)
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

fn external_overrides(producer: Producer) -> &'static [&'static str] {
    match producer {
        Producer::Vitest | Producer::Stryker => &["NODE_OPTIONS", "NODE_PATH"],
        Producer::Pytest => &[
            "PYTHONPATH",
            "PYTHONHOME",
            "PYTHONSTARTUP",
            "COVERAGE_PROCESS_START",
        ],
        Producer::CargoLlvmCov | Producer::CargoMutants => &[
            "RUSTC",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CC",
            "CXX",
            "LD",
        ],
    }
}

impl RuntimeInputs {
    pub fn capture(root: &Path, producer: Producer) -> Result<Self> {
        let mut installed_inputs = BTreeMap::new();
        let tools: &[&str] = match producer {
            Producer::Vitest | Producer::Stryker => &["node"],
            Producer::Pytest => &["python3"],
            Producer::CargoLlvmCov => &["cargo", "rustc", "cargo-llvm-cov"],
            Producer::CargoMutants => &["cargo", "rustc", "cargo-mutants"],
        };
        for tool in tools {
            if let Some(path) = executable(tool) {
                installed_inputs.insert(path.clone(), file_hash(&path)?);
            }
        }
        for dependency in dependency_roots(root, producer) {
            if dependency.exists() {
                let key = dependency
                    .strip_prefix(root)
                    .unwrap_or(&dependency)
                    .to_path_buf();
                installed_inputs.insert(key, tree_digest(&dependency)?);
            }
        }
        Ok(Self {
            hardgate_sha256: hardgate_identity()?,
            environment_sha256: environment_digest()?,
            installed_inputs,
        })
    }

    pub fn require_same(&self, current: &Self) -> Result<()> {
        anyhow::ensure!(
            self == current,
            "execution environment, installed dependencies, toolchain or Hardgate binary changed; cold evidence required"
        );
        Ok(())
    }
}

fn executable(tool: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(tool))
        .find(|path| path.is_file())
        .and_then(|path| path.canonicalize().ok())
}

fn dependency_roots(root: &Path, producer: Producer) -> Vec<PathBuf> {
    match producer {
        Producer::Vitest | Producer::Stryker => vec![root.join("node_modules")],
        Producer::Pytest => vec![root.join(".venv"), root.join("venv")],
        Producer::CargoLlvmCov | Producer::CargoMutants => {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let cargo = std::env::var_os("CARGO_HOME")
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|home| home.join(".cargo")));
            let rustup = std::env::var_os("RUSTUP_HOME")
                .map(PathBuf::from)
                .or_else(|| home.map(|home| home.join(".rustup")));
            cargo
                .into_iter()
                .flat_map(|cargo| {
                    [
                        cargo.join("registry/src"),
                        cargo.join("git/checkouts"),
                        cargo.join("config"),
                        cargo.join("config.toml"),
                    ]
                })
                .chain(
                    rustup.into_iter().flat_map(|rustup| {
                        [rustup.join("settings.toml"), rustup.join("toolchains")]
                    }),
                )
                .collect()
        }
    }
}

fn environment_digest() -> Result<String> {
    let mut values: BTreeMap<_, _> = std::env::vars_os().collect();
    // These are supervisor transport or overwritten child scratch values, not
    // project-controlled semantic inputs. All other values remain bound.
    for name in [
        "_",
        "PWD",
        "OLDPWD",
        "SHLVL",
        "HARDGATE_WORKLOAD_CHILD",
        "TMPDIR",
        "TMP",
        "TEMP",
        "HARDGATE_SCRATCH_ROOT",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "UV_CACHE_DIR",
        "npm_config_cache",
        "npm_config_store_dir",
        "pnpm_config_store_dir",
        "npm_config_verify_deps_before_run",
        "pnpm_config_verify_deps_before_run",
    ] {
        values.remove(std::ffi::OsStr::new(name));
    }
    let mut command = std::process::Command::new("hardgate-environment-identity");
    crate::resources::runtime::constrain_command(&mut command);
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                values.insert(key.to_owned(), value.to_owned());
            }
            None => {
                values.remove(key);
            }
        }
    }
    let mut hash = Sha256::new();
    for (key, value) in values {
        // OsStr bytes preserve non-UTF8 inputs; length prefixes avoid ambiguity.
        for bytes in [key.as_encoded_bytes(), value.as_encoded_bytes()] {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
    }
    Ok(hex(hash))
}

fn tree_digest(root: &Path) -> Result<String> {
    let mut pending = vec![root.to_path_buf()];
    let mut seen = BTreeSet::new();
    let mut records = BTreeMap::new();
    while let Some(path) = pending.pop() {
        crate::cancellation::check()?;
        crate::resources::check_pressure()?;
        let metadata = fs::symlink_metadata(&path)?;
        let key = path
            .strip_prefix(root)
            .context("dependency identity path escaped its root")?
            .to_path_buf();
        if metadata.is_symlink() {
            let target = path.canonicalize()?;
            let mut binding = format!(
                "symlink:{}",
                target.strip_prefix(root).unwrap_or(&target).display()
            );
            // A linked interpreter is outside the dependency tree; bind bytes.
            if path.is_file() {
                binding.push_str(&format!(":content:{}", file_hash(&path)?));
            }
            records.insert(key, binding);
        } else if metadata.is_dir() {
            if seen.insert(path.canonicalize()?) {
                for entry in fs::read_dir(&path)? {
                    pending.push(entry?.path());
                }
            }
        } else if metadata.is_file() {
            records.insert(key, file_hash(&path)?);
        } else {
            anyhow::bail!("unsupported installed dependency input: {}", path.display());
        }
    }
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&records)?);
    Ok(hex(hash))
}

fn hex(hash: Sha256) -> String {
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "runtime_inputs_tests.rs"]
mod tests;
