//! Fresh producer execution and source identity, separate from report scoring.
mod mutation_scope;
mod producer;
pub(crate) mod read_only;
mod snapshot;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Coverage,
    Mutation,
}

impl Producer {
    pub fn kind(self) -> EvidenceKind {
        match self {
            Self::CargoLlvmCov | Self::Vitest => EvidenceKind::Coverage,
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
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    producer: Producer,
    producer_version: String,
    command: Vec<Vec<String>>,
    runner_exit: i32,
    inputs: Snapshot,
    report_sha256: String,
    restoration_verified: bool,
    #[serde(default)]
    prerequisite_passed: bool,
}

/// Produce fresh evidence from an independent input copy. This operation never
/// attaches a receipt to a pre-existing report supplied by the caller.
pub fn produce(options: EvidenceOptions, context: &ConfigContext) -> CommandResult {
    ensure!(
        options.timeout_secs > 0,
        "evidence timeout must be positive"
    );
    let root = context.root.canonicalize()?;
    let destination = output_path(&root, &options)?;
    remove_receipt(&destination)?;
    let before = Snapshot::capture(&root)?;
    ensure!(
        !before.0.is_empty(),
        "evidence requires non-empty project inputs"
    );
    let workspace = workspace::EvidenceWorkspace::create(&root, &[])?;
    before.require_same(&Snapshot::capture(workspace.root())?, "producer copy")?;
    before.require_same(&Snapshot::capture(&root)?, "checkout during copy")?;
    let spec = producer::prepare(&options, workspace.root())?;
    ensure!(!spec.report.exists(), "producer report must start absent");
    let version = execute_version(&spec.version, workspace.root(), &root)?;
    let operation = if options.producer.kind() == EvidenceKind::Mutation {
        "mutation"
    } else {
        "evidence"
    };
    if let Some(tokens) = &spec.prerequisite {
        let outcome = run_command_in_copy(
            tokens,
            (workspace.root(), &root),
            Duration::from_secs(options.timeout_secs),
            "evidence",
        );
        before.require_same(
            &Snapshot::capture(workspace.root())?,
            "producer prerequisite restoration",
        )?;
        before.require_same(
            &Snapshot::capture(&root)?,
            "checkout during producer prerequisite",
        )?;
        let (exit, output) = completed_outcome(outcome, options.producer)?;
        ensure!(exit == 0, "producer prerequisite did not pass: {output}");
        eprintln!("{output}");
    }
    let outcome = run_command_in_copy(
        &spec.tokens,
        (workspace.root(), &root),
        Duration::from_secs(options.timeout_secs),
        operation,
    );
    // Compare both trees even when the producer failed. A failed or interrupted
    // run cannot leave a usable receipt from an earlier invocation.
    before.require_same(
        &Snapshot::capture(workspace.root())?,
        "producer restoration",
    )?;
    before.require_same(
        &Snapshot::capture(&root)?,
        "checkout during producer execution",
    )?;
    let (exit, output) = completed_outcome(outcome, options.producer)?;
    eprintln!("{output}");
    publish(
        ProductionOutput {
            producer: options.producer,
            spec,
            version,
            exit,
        },
        Publication {
            root: &root,
            destination: &destination,
            before,
            workspace,
            config: &context.config,
        },
    )
}

struct ProductionOutput {
    producer: Producer,
    spec: producer::CommandSpec,
    version: String,
    exit: i32,
}

struct Publication<'a> {
    root: &'a Path,
    destination: &'a Path,
    before: Snapshot,
    workspace: workspace::EvidenceWorkspace,
    config: &'a crate::config::HardgateConfig,
}

fn publish(produced: ProductionOutput, publication: Publication<'_>) -> CommandResult {
    let ProductionOutput {
        producer,
        spec,
        version,
        exit,
    } = produced;
    let Publication {
        root,
        destination,
        before,
        workspace,
        config,
    } = publication;
    let bytes = normalized_report(&spec.report, workspace.root(), producer)?;
    let temporary = destination.with_extension("pending");
    fs::write(&temporary, &bytes)?;
    let validation = validate_producer_report(
        producer,
        &temporary,
        &EvidenceInputs {
            snapshot: &before,
            config,
            root,
        },
    );
    if let Err(error) = validation {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    let prerequisite_passed = spec.prerequisite.is_some();
    let receipt = Receipt {
        schema_version: 1,
        producer,
        producer_version: version,
        command: spec
            .prerequisite
            .into_iter()
            .chain(std::iter::once(spec.tokens))
            .collect(),
        runner_exit: exit,
        inputs: before,
        report_sha256: file_hash(&temporary)?,
        restoration_verified: true,
        prerequisite_passed,
    };
    workspace.close()?;
    receipt.inputs.require_same(
        &Snapshot::capture(root)?,
        "checkout before evidence publication",
    )?;
    fs::rename(&temporary, destination)?;
    crate::commands::outcome::write_atomic_file(
        &receipt_path(destination),
        &serde_json::to_string_pretty(&receipt)?,
    )?;
    eprintln!("source-bound evidence: {}", destination.display());
    Ok(if exit == 0 {
        CommandOutcome::Passed
    } else {
        CommandOutcome::Violations
    })
}

/// Verify freshness and producer identity before accepting report evidence.
/// This is a local execution receipt, not a signature or a claim of hermeticity.
pub fn verify(
    root: &Path,
    report: &Path,
    kind: EvidenceKind,
    config: &crate::config::HardgateConfig,
) -> Result<()> {
    let receipt: Receipt =
        serde_json::from_slice(&fs::read(receipt_path(report)).with_context(|| {
            format!(
                "missing source-bound receipt for {}; regenerate with `hardgate evidence`",
                report.display()
            )
        })?)?;
    ensure!(
        receipt.schema_version == 1 && receipt.restoration_verified,
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
    receipt
        .inputs
        .require_same(&Snapshot::capture(root)?, "stale evidence")?;
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

fn execute_version(tokens: &[String], root: &Path, original: &Path) -> Result<String> {
    match run_command_in_copy(
        tokens,
        (root, original),
        Duration::from_secs(30),
        "evidence",
    ) {
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
        Ok(content.into_bytes())
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
