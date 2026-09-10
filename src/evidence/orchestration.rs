use super::{EvidenceKind, EvidenceOptions, Producer, Receipt};
use crate::config::ConfigContext;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceMode {
    /// Reuse only authenticated evidence for identical bound inputs.
    Reuse,
    /// Run every configured producer from a fresh independent copy.
    Cold,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRun {
    #[serde(default)]
    pub kind: Option<EvidenceKind>,
    pub name: String,
    pub status: String,
    pub duration_ms: u128,
    pub report: PathBuf,
    pub detail: Option<String>,
}

pub(crate) fn run_configured(context: &ConfigContext, mode: EvidenceMode) -> Vec<EvidenceRun> {
    if let Err(error) = preflight(context) {
        return vec![EvidenceRun {
            kind: None,
            name: "preflight".into(),
            status: "failed".into(),
            duration_ms: 0,
            report: context.root.clone(),
            detail: Some(format!("{error:#}")),
        }];
    }
    let mut runs = Vec::new();
    for (name, config) in &context.config.evidence.producers {
        if !enabled(context, config.producer.kind()) {
            continue;
        }
        let start = Instant::now();
        let report = context.root.join(format!(
            ".hardgate/evidence/{name}.{}",
            if config.producer.kind() == EvidenceKind::Coverage {
                "lcov"
            } else {
                "json"
            }
        ));
        let _phase = crate::engines::process::phase::set(&format!("evidence:{name}"));
        let (status, detail) = run_one(context, mode, name, &report);
        crate::engines::process::diagnostic(format_args!(
            "hardgate: evidence configuration={name} status={status} elapsed={}ms",
            start.elapsed().as_millis()
        ));
        runs.push(EvidenceRun {
            kind: Some(config.producer.kind()),
            name: name.clone(),
            status: status.into(),
            duration_ms: start.elapsed().as_millis(),
            report,
            detail,
        });
        if crate::cancellation::signal().is_some() {
            break;
        }
    }
    runs
}

fn reusable(context: &ConfigContext, report: &Path, producer: Producer) -> Result<()> {
    ensure!(
        super::runtime_inputs::can_reuse(&context.root, producer),
        "installed runtime or external tool overrides require cold execution"
    );
    super::verify(&context.root, report, producer.kind(), &context.config)?;
    let receipt: Receipt = serde_json::from_slice(&std::fs::read(super::receipt_path(report))?)?;
    let original = receipt.runtime_inputs.context("producer has no reusable installed-runtime binding; global Python environments and external tool overrides always run cold")?;
    original.require_same(&super::runtime_inputs::RuntimeInputs::capture(
        &context.root,
        producer,
    )?)
}

pub(crate) fn baseline_for(
    context: &ConfigContext,
    command: &str,
    runs: &[EvidenceRun],
) -> Option<PathBuf> {
    let tokens = crate::engines::orchestration::shell_words_split(command);
    for run in runs
        .iter()
        .filter(|run| run.status == "produced" || run.status == "reused")
    {
        let bytes = std::fs::read(super::receipt_path(&run.report)).ok()?;
        let receipt: Receipt = serde_json::from_slice(&bytes).ok()?;
        if matches_baseline(receipt.prerequisite_passed, &receipt.command, &tokens)
            && reusable(context, &run.report, receipt.producer).is_ok()
        {
            return Some(run.report.clone());
        }
    }
    None
}

fn matches_baseline(passed: bool, executed: &[Vec<String>], requested: &[String]) -> bool {
    passed
        && !requested.is_empty()
        && executed.len() >= 2
        && executed.first().is_some_and(|command| command == requested)
}

#[cfg(test)]
#[path = "orchestration_tests.rs"]
mod tests;

fn enabled(context: &ConfigContext, kind: EvidenceKind) -> bool {
    match kind {
        EvidenceKind::Coverage => context.config.coverage.enabled,
        EvidenceKind::Mutation => context.config.mutation.enabled,
    }
}

fn preflight(context: &ConfigContext) -> Result<()> {
    crate::resources::runtime::require()?;
    ensure!(
        context.config.coverage.enabled || context.config.mutation.enabled,
        "evidence orchestration requires an enabled coverage or mutation engine"
    );
    for kind in [EvidenceKind::Coverage, EvidenceKind::Mutation] {
        if !enabled(context, kind) {
            continue;
        }
        ensure!(
            context
                .config
                .evidence
                .producers
                .values()
                .any(|producer| producer.producer.kind() == kind),
            "enabled {kind:?} engine requires named [evidence.producers.NAME] configurations for orchestration"
        );
    }
    Ok(())
}

fn run_one(
    context: &ConfigContext,
    mode: EvidenceMode,
    name: &str,
    report: &Path,
) -> (&'static str, Option<String>) {
    let config = &context.config.evidence.producers[name];
    let reusable = if mode == EvidenceMode::Reuse {
        reusable(context, report, config.producer)
    } else {
        Err(anyhow::anyhow!("cold execution requested"))
    };
    match reusable {
        Ok(()) => (
            "reused",
            Some("authenticated report and identical local execution inputs".into()),
        ),
        Err(reason) => match super::produce(
            EvidenceOptions {
                producer: config.producer,
                name: None,
                toolchain: None,
                timeout_secs: config.timeout_secs,
                args: vec![],
                producer_config: Some(name.to_string()),
            },
            context,
        ) {
            Ok(_) => ("produced", Some(format!("cold execution: {reason:#}"))),
            Err(error) => ("failed", Some(format!("{error:#}"))),
        },
    }
}
