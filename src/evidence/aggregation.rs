//! Evidence partitions are disjoint and explicit. A report is never proof of
//! exhaustive scope merely because every path it happens to contain is valid.
use super::{
    EvidenceKind, Producer, Receipt,
    partitions::{self, Partition},
};
use crate::config::{HardgateConfig, MutationScope};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn verify_set(
    root: &Path,
    reports: &[String],
    kind: EvidenceKind,
    config: &HardgateConfig,
) -> Result<()> {
    ensure!(!reports.is_empty(), "no evidence reports configured");
    let mut owners = BTreeMap::new();
    let mut partitions_seen = BTreeSet::new();
    for report in reports {
        let path = root.join(report);
        super::verify(root, &path, kind, config)?;
        let receipt: Receipt = serde_json::from_slice(&std::fs::read(super::receipt_path(&path))?)?;
        if let Some(partition) = &receipt.partition {
            ensure!(
                partition.config.scope == MutationScope::Exhaustive,
                "evidence partition `{}` is a sample; full verification is incomplete",
                partition.name
            );
            ensure!(
                partitions_seen.insert(partition.name.clone()),
                "duplicate evidence partition `{}`",
                partition.name
            );
        } else {
            ensure!(
                kind != EvidenceKind::Mutation,
                "unnamed mutation evidence is a sample; declare an exhaustive named producer partition for full verification"
            );
        }
        for source in reported_sources(receipt.producer, &path, root, config)? {
            ensure!(
                owners.insert(source.clone(), report.clone()).is_none(),
                "overlapping evidence records for {}; partitions must be disjoint",
                source.display()
            );
        }
    }
    let configured: BTreeSet<_> = config
        .evidence
        .producers
        .iter()
        .filter(|(_, p)| p.producer.kind() == kind)
        .map(|(name, _)| name.clone())
        .collect();
    ensure!(
        configured == partitions_seen,
        "missing or unexpected named evidence partitions: expected {configured:?}, received {partitions_seen:?}"
    );
    if !configured.is_empty() {
        for source in partitions::source_inventory(root, config)? {
            ensure!(
                owners.contains_key(&source),
                "exhaustive {:?} evidence is missing source {}",
                kind,
                source.display()
            );
        }
    }
    Ok(())
}

pub(super) fn validate_partition_report(
    producer: Producer,
    report: &Path,
    context: (&Path, &HardgateConfig),
    partition: &Partition,
) -> Result<()> {
    let (root, config) = context;
    let current = config
        .evidence
        .producers
        .get(&partition.name)
        .context("receipt names an unconfigured evidence partition")?;
    ensure!(
        current == &partition.config && current.producer == producer,
        "evidence producer configuration changed"
    );
    let expected = partitions::selected_sources(root, config, current)?;
    ensure!(
        expected == partition.sources,
        "evidence partition source inventory changed"
    );
    let records = reported_sources(producer, report, root, config)?;
    let expected: BTreeSet<_> = expected.into_iter().collect();
    ensure!(
        records.is_subset(&expected),
        "producer reported executable source outside partition `{}`",
        partition.name
    );
    if current.scope == MutationScope::Exhaustive {
        if producer == Producer::Stryker {
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
            ensure!(
                value.get("hardgate_scope").is_some(),
                "exhaustive Stryker evidence requires native source inventory and complete mutation-plan evidence"
            );
        }
        let missing: Vec<_> = expected.difference(&records).collect();
        ensure!(
            missing.is_empty(),
            "partition `{}` has no producer records for {missing:?}; exhaustive scope is unproven (zero-mutant files require native scope evidence)",
            partition.name
        );
    }
    Ok(())
}

fn reported_sources(
    producer: Producer,
    path: &Path,
    root: &Path,
    config: &HardgateConfig,
) -> Result<BTreeSet<PathBuf>> {
    if producer.kind() == EvidenceKind::Coverage {
        let scorer = crate::engines::CoverageScorer::new(&config.coverage);
        return scorer
            .parse_lcov_for_project(path, root, config)?
            .keys()
            .map(|path| {
                crate::engines::coverage::normalized_repository_key(path, root)
                    .map(PathBuf::from)
                    .context("coverage path is outside repository")
            })
            .collect();
    }
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    if producer == Producer::Stryker {
        if value.get("hardgate_scope").is_some() {
            let policy = super::inputs::InputPolicy::new(root, config)?;
            let inputs = super::Snapshot::capture_with(root, &policy)?;
            return super::stryker_scope::sources(&value, &inputs)?
                .context("missing native Stryker scope");
        }
        Ok(value["files"]
            .as_object()
            .context("missing Stryker source records")?
            .keys()
            .map(PathBuf::from)
            .collect())
    } else {
        value["outcomes"]
            .as_array()
            .context("missing cargo-mutants outcomes")?
            .iter()
            .filter_map(|outcome| outcome.pointer("/scenario/Mutant"))
            .map(|mutant| {
                mutant["file"]
                    .as_str()
                    .map(PathBuf::from)
                    .context("missing mutant file")
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "aggregation_tests.rs"]
mod tests;
