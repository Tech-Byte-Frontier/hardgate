//! Fresh producer execution and source identity, separate from report scoring.
mod aggregation;
mod authentication;
mod orchestration;
mod runtime_inputs;
mod rust_scope;
pub use orchestration::{EvidenceMode, EvidenceRun};
pub(crate) use orchestration::{baseline_for, run_configured};
mod environment;
mod execution;
pub use execution::produce;
mod publication;
use publication::publish;
mod inputs;
mod mutation_scope;
mod partitions;
mod producer_rust;
pub use aggregation::verify_set;
pub use partitions::reports;
mod producer;
mod python;
mod python_report;
pub(crate) mod read_only;
mod read_only_publication;
mod snapshot;
mod stryker_scope;
pub(crate) mod temporary;
pub use temporary::configure_root as configure_scratch_root;
mod workspace;

use crate::commands::{CommandOutcome, CommandResult};
use crate::config::ConfigContext;
use crate::engines::process::{ProcessOutcome, run_command_in_copy};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use snapshot::{Snapshot, file_hash};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Producer {
    CargoLlvmCov,
    Vitest,
    CargoMutants,
    Stryker,
    Pytest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Coverage,
    Mutation,
}

impl Producer {
    pub fn name(self) -> &'static str {
        match self {
            Self::CargoLlvmCov => "cargo-llvm-cov",
            Self::Vitest => "vitest",
            Self::CargoMutants => "cargo-mutants",
            Self::Stryker => "stryker",
            Self::Pytest => "pytest",
        }
    }
    pub fn kind(self) -> EvidenceKind {
        match self {
            Self::CargoLlvmCov | Self::Vitest | Self::Pytest => EvidenceKind::Coverage,
            Self::CargoMutants | Self::Stryker => EvidenceKind::Mutation,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvidenceOptions {
    pub producer: Producer,
    pub name: Option<String>,
    pub toolchain: Option<String>,
    pub timeout_secs: u64,
    pub args: Vec<String>,
    pub producer_config: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    root: PathBuf,
    producer: Producer,
    producer_version: String,
    command: Vec<Vec<String>>,
    runner_exit: i32,
    inputs: Snapshot,
    report_sha256: String,
    restoration_verified: bool,
    #[serde(default)]
    prerequisite_passed: bool,
    /// Until this owned job is removed, publication/cleanup is incomplete.
    #[serde(default)]
    workspace: Option<PathBuf>,
    #[serde(default)]
    partition: Option<partitions::Partition>,
    #[serde(default)]
    runtime_inputs: Option<runtime_inputs::RuntimeInputs>,
}

struct ProductionOutput {
    producer: Producer,
    spec: producer::CommandSpec,
    version: String,
    exit: i32,
    partition: Option<partitions::Partition>,
    runtime_inputs: Option<runtime_inputs::RuntimeInputs>,
}

struct Publication<'a> {
    input_policy: inputs::InputPolicy,
    root: &'a Path,
    destination: &'a Path,
    before: Snapshot,
    workspace: workspace::EvidenceWorkspace,
    config: &'a crate::config::HardgateConfig,
}

/// Verify freshness and producer identity before accepting report evidence.
/// Authentication comes from the protected local registry, not the editable
/// report sidecar. This is not portable signing or a claim of hermeticity.
pub fn verify(
    root: &Path,
    report: &Path,
    kind: EvidenceKind,
    config: &crate::config::HardgateConfig,
) -> Result<()> {
    let receipt_bytes = fs::read(receipt_path(report)).with_context(|| {
        format!(
            "missing source-bound receipt for {}; regenerate with `hardgate evidence`",
            report.display()
        )
    })?;
    authentication::verify(&receipt_bytes, root, report)?;
    let receipt: Receipt = serde_json::from_slice(&receipt_bytes)?;
    ensure!(
        receipt.root == root.canonicalize()?,
        "receipt belongs to a different source checkout"
    );
    ensure!(
        receipt.workspace.as_ref().is_none_or(|path| !path.exists()),
        "evidence publication or workspace cleanup is incomplete; inspect {}",
        receipt
            .workspace
            .as_deref()
            .unwrap_or(Path::new("<unknown>"))
            .display()
    );
    ensure!(
        receipt.schema_version == 2 && receipt.restoration_verified,
        "unsupported or unrestored evidence receipt"
    );
    ensure!(
        receipt.producer.kind() == kind,
        "wrong evidence producer for this report"
    );
    ensure!(
        !receipt.producer_version.trim().is_empty() && !receipt.command.is_empty(),
        "evidence receipt lacks producer execution identity"
    );
    if receipt.producer == Producer::CargoMutants {
        ensure!(
            receipt.prerequisite_passed
                && receipt.command.len() >= 2
                && receipt.command[0].iter().any(|arg| arg == "test")
                && receipt.command[0].iter().any(|arg| arg == "--workspace"),
            "cargo-mutants receipt lacks a successful full-workspace baseline; regenerate evidence"
        );
    }
    ensure!(
        allowed_exit(receipt.producer, receipt.runner_exit),
        "producer execution was incomplete"
    );
    ensure!(
        file_hash(report)? == receipt.report_sha256,
        "report bytes changed after producer execution; regenerate evidence"
    );
    let input_policy = inputs::InputPolicy::new(root, config)?;
    receipt.inputs.require_same(
        &Snapshot::capture_with(root, &input_policy)?,
        "stale evidence",
    )?;
    if let Some(partition) = &receipt.partition {
        aggregation::validate_partition_report(
            receipt.producer,
            report,
            (root, config),
            partition,
        )?;
    }
    validate_producer_report(
        receipt.producer,
        report,
        &EvidenceInputs {
            snapshot: &receipt.inputs,
            config,
            root,
        },
    )
}

fn output_path(root: &Path, options: &EvidenceOptions) -> Result<PathBuf> {
    let name = options
        .name
        .as_deref()
        .unwrap_or(match options.producer.kind() {
            EvidenceKind::Coverage => "coverage",
            EvidenceKind::Mutation => "mutation",
        });
    ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "evidence name must contain only letters, digits, '-' or '_'"
    );
    let directory = output_directory(root)?;
    let extension = if options.producer.kind() == EvidenceKind::Coverage {
        "lcov"
    } else {
        "json"
    };
    let destination = directory.join(format!("{name}.{extension}"));
    for path in [
        &destination,
        &receipt_path(&destination),
        &destination.with_extension("pending"),
    ] {
        if let Ok(metadata) = fs::symlink_metadata(path) {
            ensure!(
                metadata.is_file() && !metadata.is_symlink(),
                "evidence artifact must be a regular file: {}",
                path.display()
            );
        }
    }
    Ok(destination)
}

fn output_directory(root: &Path) -> Result<PathBuf> {
    let mut directory = root.to_path_buf();
    for part in [".hardgate", "evidence"] {
        directory.push(part);
        match fs::symlink_metadata(&directory) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !metadata.is_symlink(),
                "evidence output directory must not be a symlink or file: {}",
                directory.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&directory)?
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(directory)
}

fn receipt_path(report: &Path) -> PathBuf {
    let mut name = report.as_os_str().to_os_string();
    name.push(".hardgate.json");
    PathBuf::from(name)
}
fn remove_receipt(report: &Path) -> Result<()> {
    match fs::remove_file(receipt_path(report)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn execute_version(
    tokens: &[String],
    root: &Path,
    original: &Path,
    timeout: Duration,
) -> Result<String> {
    match run_command_in_copy(tokens, (root, original), timeout, "evidence") {
        ProcessOutcome::Completed { status, output }
            if status.success() && !output.trim().is_empty() =>
        {
            Ok(output.trim().to_string())
        }
        outcome => bail!(
            "producer is unavailable or cannot report its version; install the declared tool: {outcome:?}"
        ),
    }
}

fn allowed_exit(producer: Producer, code: i32) -> bool {
    code == 0
        || (producer == Producer::CargoMutants && matches!(code, 2 | 3))
        || (producer == Producer::Stryker && code == 1)
}

fn completed_outcome(outcome: ProcessOutcome, producer: Producer) -> Result<(i32, String)> {
    match outcome {
        ProcessOutcome::Completed { status, output }
            if status
                .code()
                .is_some_and(|code| allowed_exit(producer, code)) =>
        {
            Ok((status.code().unwrap_or(2), output))
        }
        outcome => bail!("evidence producer did not complete successfully: {outcome:?}"),
    }
}

fn normalized_report(path: &Path, root: &Path, producer: Producer) -> Result<Vec<u8>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("producer did not create required report {}", path.display()))?;
    ensure!(
        !content.trim().is_empty(),
        "producer created an empty report"
    );
    if producer.kind() == EvidenceKind::Coverage {
        let content = if producer == Producer::Pytest {
            python_report::normalize(&content, &path.with_extension("native.json"))?
        } else {
            content
        };
        let mut output = String::new();
        for line in content.lines() {
            if let Some(path) = line.strip_prefix("SF:") {
                let path = Path::new(path);
                output.push_str("SF:");
                output.push_str(&path.strip_prefix(root).unwrap_or(path).to_string_lossy());
            } else {
                output.push_str(line);
            }
            output.push('\n');
        }
        Ok(output.into_bytes())
    } else {
        if producer == Producer::Stryker {
            stryker_scope::merge(&content, path)
        } else {
            Ok(content.into_bytes())
        }
    }
}

struct EvidenceInputs<'a> {
    snapshot: &'a Snapshot,
    config: &'a crate::config::HardgateConfig,
    root: &'a Path,
}

fn validate_producer_report(
    producer: Producer,
    path: &Path,
    inputs: &EvidenceInputs<'_>,
) -> Result<()> {
    match producer.kind() {
        EvidenceKind::Coverage => {
            let scorer =
                crate::engines::CoverageScorer::new(&crate::config::CoverageConfig::default());
            let map = scorer.parse_lcov(path)?;
            ensure!(!map.is_empty(), "coverage producer reported no files");
        }
        EvidenceKind::Mutation => {
            let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
            match producer {
                Producer::CargoMutants => ensure!(
                    value
                        .get("outcomes")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|outcomes| outcomes
                            .iter()
                            .any(|outcome| outcome.get("scenario")
                                == Some(&serde_json::Value::String("Baseline".into())))),
                    "cargo-mutants evidence requires structured baseline and mutant outcomes"
                ),
                Producer::Stryker => validate_stryker_sources(&value, inputs.snapshot)?,
                _ => unreachable!(),
            }
            mutation_scope::validate(&value, producer, inputs)?;
            // Report integrity and score remain separate from source identity.
            // Evaluation errors reject malformed reports; violations are kept for
            // the configured acceptance gate rather than hidden by the producer.
            crate::engines::MutationGatekeeper::new(&crate::config::MutationConfig::default())
                .evaluate_report(path)?;
        }
    }
    Ok(())
}

fn validate_stryker_sources(value: &serde_json::Value, inputs: &Snapshot) -> Result<()> {
    use sha2::{Digest, Sha256};
    stryker_scope::sources(value, inputs)?;
    let files = value
        .get("files")
        .and_then(serde_json::Value::as_object)
        .context("Stryker report requires source files")?;
    for (name, file) in files {
        let source = file
            .get("source")
            .and_then(serde_json::Value::as_str)
            .context("Stryker report lacks original source bytes")?;
        let hash: String = Sha256::digest(source.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        ensure!(
            inputs.0.get(Path::new(name)) == Some(&hash),
            "Stryker reported source differs from executed inputs: {name}"
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod publication_tests;
