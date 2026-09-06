use super::RecordBuilder;
use anyhow::{Result, bail};
use std::collections::HashSet;

enum MetricCounterKind {
    Function,
    Branch,
}

fn validate_metric_counts(
    builder: &RecordBuilder,
    required: bool,
    kind: MetricCounterKind,
) -> Result<()> {
    let pair = match kind {
        MetricCounterKind::Function => CounterPair {
            found_tag: "FNF",
            hit_tag: "FNH",
            label: "function counts",
            required_label: "required FNF/FNH counts",
            found: builder.coverage.functions_found,
            hit: builder.coverage.functions_hit,
            exceeds: "LCOV FNH exceeds FNF",
        },
        MetricCounterKind::Branch => CounterPair {
            found_tag: "BRF",
            hit_tag: "BRH",
            label: "branch counts",
            required_label: "required BRF/BRH counts",
            found: builder.coverage.branches_found,
            hit: builder.coverage.branches_hit,
            exceeds: "LCOV BRH exceeds BRF",
        },
    };
    validate_counter_pair(builder, required, pair)
}

pub(super) fn validate_function_counts(builder: &RecordBuilder, required: bool) -> Result<()> {
    validate_metric_counts(builder, required, MetricCounterKind::Function)
}

pub(super) fn validate_branch_counts(builder: &RecordBuilder, required: bool) -> Result<()> {
    validate_metric_counts(builder, required, MetricCounterKind::Branch)
}

struct CounterPair {
    found_tag: &'static str,
    hit_tag: &'static str,
    label: &'static str,
    required_label: &'static str,
    found: usize,
    hit: usize,
    exceeds: &'static str,
}

fn validate_counter_pair(builder: &RecordBuilder, required: bool, pair: CounterPair) -> Result<()> {
    validate_pair(
        &builder.seen_counts,
        pair.found_tag,
        pair.hit_tag,
        pair.label,
    )?;
    if required {
        require_counts(
            &builder.seen_counts,
            &[pair.found_tag, pair.hit_tag],
            pair.required_label,
        )?;
    }
    if pair.hit > pair.found {
        bail!("{}", pair.exceeds);
    }
    Ok(())
}

pub(super) fn validate_pair(
    seen_counts: &HashSet<&'static str>,
    first: &'static str,
    second: &'static str,
    label: &str,
) -> Result<()> {
    if seen_counts.contains(first) == seen_counts.contains(second) {
        Ok(())
    } else {
        bail!("LCOV {first}/{second} {label} must be paired")
    }
}

pub(super) fn require_counts(
    seen_counts: &HashSet<&'static str>,
    tags: &[&'static str],
    label: &str,
) -> Result<()> {
    if tags.iter().all(|tag| seen_counts.contains(tag)) {
        Ok(())
    } else {
        bail!("LCOV source record is missing {label}")
    }
}
