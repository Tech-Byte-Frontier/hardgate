//! Pure comparison of current static debt with a merge-base report.

mod legacy_hunk;

use crate::diagnostics::GateReport;
use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, DeadCodeViolation, InvariantViolation,
    SuppressionViolation,
};
use crate::git_evidence::ChangeSet;
use legacy_hunk::LegacyFinding;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
/// Result of applying a legacy ratchet to one report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LegacyRatchetOutcome {
    pub merge_base: String,
    pub grandfathered: usize,
    pub retained: usize,
    pub advisories: Vec<String>,
}
pub type LegacyRatchetSummary = LegacyRatchetOutcome;
/// Compare `current` with `baseline` and mutate the current report in place.
/// Only non-worsened static debt is grandfathered; evidence vectors remain blocking.
pub fn apply_legacy_ratchet(
    current: &mut GateReport,
    baseline: &GateReport,
    changes: &ChangeSet,
) -> LegacyRatchetOutcome {
    let mut advisories = Vec::new();
    advisories.extend(ratchet_numeric(
        &mut current.budget_violations,
        &baseline.budget_violations,
        changes,
        (
            budget_key,
            |violation: &BudgetViolation| violation.actual,
            format_budget_advisory,
        ),
    ));
    advisories.extend(ratchet_multiset(
        &mut current.suppression_violations,
        &baseline.suppression_violations,
        changes,
        (
            suppression_key,
            format_suppression_advisory,
            |_: &SuppressionViolation| true,
        ),
    ));
    advisories.extend(ratchet_numeric(
        &mut current.complexity_violations,
        &baseline.complexity_violations,
        changes,
        (
            complexity_key,
            |violation: &ComplexityViolation| violation.actual,
            format_complexity_advisory,
        ),
    ));
    advisories.extend(ratchet_multiset(
        &mut current.invariant_violations,
        &baseline.invariant_violations,
        changes,
        (
            invariant_key,
            format_invariant_advisory,
            |_: &InvariantViolation| true,
        ),
    ));
    advisories.extend(ratchet_multiset(
        &mut current.clone_violations,
        &baseline.clone_violations,
        changes,
        (
            clone_key,
            format_clone_advisory,
            |violation: &CloneViolation| !violation.fingerprint.is_empty(),
        ),
    ));
    advisories.extend(ratchet_multiset(
        &mut current.dead_code_violations,
        &baseline.dead_code_violations,
        changes,
        (
            dead_code_key,
            format_dead_code_advisory,
            |_: &DeadCodeViolation| true,
        ),
    ));

    advisories.sort();
    current.advisories.extend(advisories.iter().cloned());
    annotate_retained(current, changes);
    current.passed = current.total_violations() == 0;

    LegacyRatchetOutcome {
        merge_base: changes.merge_base.clone(),
        grandfathered: advisories.len(),
        retained: current.total_violations(),
        advisories,
    }
}

pub fn ratchet_report(
    current: &mut GateReport,
    baseline: &GateReport,
    changes: &ChangeSet,
) -> LegacyRatchetOutcome {
    apply_legacy_ratchet(current, baseline, changes)
}
type ComplexityKey = (PathBuf, String, String);
type SuppressionKey = (PathBuf, String, String);
type InvariantKey = (PathBuf, String, String, String, String);
type CloneKey = (PathBuf, PathBuf, String);
type DeadCodeKey = (PathBuf, String, Option<String>);
fn budget_key(
    violation: &BudgetViolation,
    lineage: &BTreeMap<PathBuf, PathBuf>,
) -> (PathBuf, String) {
    (
        canonical_path(&violation.file, lineage),
        violation.metric.clone(),
    )
}
fn ratchet_numeric<T, K, N, Key, Actual, Advisory>(
    current: &mut Vec<T>,
    baseline: &[T],
    changes: &ChangeSet,
    ops: (Key, Actual, Advisory),
) -> Vec<String>
where
    T: LegacyFinding,
    K: Ord,
    N: Copy + PartialOrd,
    Key: Fn(&T, &BTreeMap<PathBuf, PathBuf>) -> K,
    Actual: Fn(&T) -> N,
    Advisory: Fn(&T, N, &ChangeSet) -> String,
{
    let (key, actual, advisory) = ops;
    let empty_lineage = BTreeMap::new();
    let mut available: BTreeMap<K, Vec<N>> = BTreeMap::new();
    for violation in baseline {
        available
            .entry(key(violation, &empty_lineage))
            .or_default()
            .push(actual(violation));
    }
    for values in available.values_mut() {
        values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    }
    let mut advisories = Vec::new();
    current.retain(|violation| {
        if violation.attributable(changes) {
            return true;
        }
        let key = key(violation, &changes.rename_lineage);
        let Some(values) = available.get_mut(&key) else {
            return true;
        };
        let current_actual = actual(violation);
        let Some(index) = values
            .iter()
            .position(|baseline_actual| current_actual <= *baseline_actual)
        else {
            return true;
        };
        let baseline_actual = values.remove(index);
        advisories.push(advisory(violation, baseline_actual, changes));
        false
    });
    advisories
}
fn ratchet_multiset<T, K, Key, Advisory, Eligible>(
    current: &mut Vec<T>,
    baseline: &[T],
    changes: &ChangeSet,
    ops: (Key, Advisory, Eligible),
) -> Vec<String>
where
    T: LegacyFinding,
    K: Ord,
    Key: Fn(&T, &BTreeMap<PathBuf, PathBuf>) -> K,
    Advisory: Fn(&T, &ChangeSet) -> String,
    Eligible: Fn(&T) -> bool,
{
    let (key, advisory, eligible) = ops;
    let empty_lineage = BTreeMap::new();
    let mut available: BTreeMap<K, usize> = BTreeMap::new();
    for violation in baseline.iter().filter(|violation| eligible(violation)) {
        *available.entry(key(violation, &empty_lineage)).or_default() += 1;
    }
    let mut advisories = Vec::new();
    current.retain(|violation| {
        if !eligible(violation) || violation.attributable(changes) {
            return true;
        }
        let key = key(violation, &changes.rename_lineage);
        let Some(count) = available.get_mut(&key) else {
            return true;
        };
        if *count == 0 {
            return true;
        }
        *count -= 1;
        advisories.push(advisory(violation, changes));
        false
    });
    advisories
}
fn suppression_key(
    violation: &SuppressionViolation,
    lineage: &BTreeMap<PathBuf, PathBuf>,
) -> SuppressionKey {
    (
        canonical_path(&violation.file, lineage),
        violation.token.clone(),
        normalize_line(&violation.line_content),
    )
}
fn complexity_key(
    violation: &ComplexityViolation,
    lineage: &BTreeMap<PathBuf, PathBuf>,
) -> ComplexityKey {
    (
        canonical_path(&violation.file, lineage),
        violation.function_name.clone(),
        violation.metric.clone(),
    )
}
fn invariant_key(
    violation: &InvariantViolation,
    lineage: &BTreeMap<PathBuf, PathBuf>,
) -> InvariantKey {
    (
        canonical_path(&violation.file, lineage),
        violation.rule_name.clone(),
        violation.violation_type.clone(),
        violation.offending_target.clone(),
        normalize_line(&violation.line_content),
    )
}
fn clone_key(violation: &CloneViolation, lineage: &BTreeMap<PathBuf, PathBuf>) -> CloneKey {
    let left = canonical_path(&violation.file_a, lineage);
    let right = canonical_path(&violation.file_b, lineage);
    if left <= right {
        (left, right, violation.fingerprint.clone())
    } else {
        (right, left, violation.fingerprint.clone())
    }
}
fn dead_code_key(
    violation: &DeadCodeViolation,
    lineage: &BTreeMap<PathBuf, PathBuf>,
) -> DeadCodeKey {
    (
        canonical_path(&violation.file, lineage),
        violation.violation_type.clone(),
        violation.symbol.clone(),
    )
}
fn canonical_path(path: &Path, lineage: &BTreeMap<PathBuf, PathBuf>) -> PathBuf {
    let mut current = path.to_path_buf();
    let mut visited = BTreeSet::new();
    while visited.insert(current.clone()) {
        let Some(previous) = lineage.get(&current) else {
            break;
        };
        current = previous.clone();
    }
    current
}
fn normalize_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn format_budget_advisory(
    violation: &BudgetViolation,
    baseline_actual: usize,
    changes: &ChangeSet,
) -> String {
    format!(
        "legacy ratchet: grandfathered budget debt at `{}` ({}, current={} <= baseline={}; merge-base={})",
        violation.file.display(),
        violation.metric,
        violation.actual,
        baseline_actual,
        changes.merge_base
    )
}
fn format_suppression_advisory(violation: &SuppressionViolation, changes: &ChangeSet) -> String {
    format!(
        "legacy ratchet: grandfathered suppression debt at `{}` (token={}; merge-base={})",
        violation.file.display(),
        violation.token,
        changes.merge_base
    )
}
fn format_complexity_advisory(
    violation: &ComplexityViolation,
    baseline_actual: f64,
    changes: &ChangeSet,
) -> String {
    format!(
        "legacy ratchet: grandfathered complexity debt at `{}::{}` ({}, current={} <= baseline={}; merge-base={})",
        violation.file.display(),
        violation.function_name,
        violation.metric,
        format_number(violation.actual),
        format_number(baseline_actual),
        changes.merge_base
    )
}
fn format_invariant_advisory(violation: &InvariantViolation, changes: &ChangeSet) -> String {
    format!(
        "legacy ratchet: grandfathered invariant debt at `{}` (rule={}, type={}, target={}; merge-base={})",
        violation.file.display(),
        violation.rule_name,
        violation.violation_type,
        violation.offending_target,
        changes.merge_base
    )
}
fn format_clone_advisory(violation: &CloneViolation, changes: &ChangeSet) -> String {
    format!(
        "legacy ratchet: grandfathered clone debt between `{}` and `{}` (fingerprint={}; merge-base={})",
        violation.file_a.display(),
        violation.file_b.display(),
        violation.fingerprint,
        changes.merge_base
    )
}
fn format_dead_code_advisory(violation: &DeadCodeViolation, changes: &ChangeSet) -> String {
    let symbol = violation.symbol.as_deref().unwrap_or("<none>");
    format!(
        "legacy ratchet: grandfathered dead-code debt at `{}` (type={}, symbol={}; merge-base={})",
        violation.file.display(),
        violation.violation_type,
        symbol,
        changes.merge_base
    )
}
fn format_number(value: f64) -> String {
    let rendered = format!("{value:.3}");
    rendered
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
fn annotate_retained(report: &mut GateReport, changes: &ChangeSet) {
    for violation in &mut report.budget_violations {
        append_context(
            &mut violation.message,
            file_context(&violation.file, changes),
        );
    }
    for violation in &mut report.suppression_violations {
        append_context(
            &mut violation.message,
            line_context(&violation.file, violation.line_number, changes),
        );
    }
    for violation in &mut report.complexity_violations {
        append_context(
            &mut violation.message,
            line_context(&violation.file, violation.line_number, changes),
        );
    }
    for violation in &mut report.invariant_violations {
        append_context(
            &mut violation.message,
            line_context(&violation.file, violation.line_number, changes),
        );
    }
    for violation in &mut report.clone_violations {
        let contexts = [
            range_context(&violation.file_a, violation.lines_a, changes),
            range_context(&violation.file_b, violation.lines_b, changes),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        if !contexts.is_empty() {
            append_context(
                &mut violation.message,
                Some(format!("changed clone ranges: {}", contexts.join(", "))),
            );
        }
    }
    for violation in &mut report.coverage_violations {
        append_context(
            &mut violation.message,
            file_context(&violation.file, changes),
        );
    }
    for violation in &mut report.mutation_violations {
        append_context(
            &mut violation.message,
            file_context(&violation.report_file, changes),
        );
    }
    for violation in &mut report.dead_code_violations {
        let context = violation
            .line_number
            .and_then(|line| line_context(&violation.file, line, changes))
            .or_else(|| file_context(&violation.file, changes));
        append_context(&mut violation.message, context);
    }
}
fn append_context(message: &mut String, context: Option<String>) {
    let Some(context) = context else {
        return;
    };
    if message.contains("[changed ") {
        return;
    }
    if !message.is_empty() {
        message.push(' ');
    }
    message.push('[');
    message.push_str(&context);
    message.push(']');
}
fn file_context(path: &Path, changes: &ChangeSet) -> Option<String> {
    if let Some(lines) = changes.changed_lines.get(path)
        && let Some(first) = lines.iter().next()
    {
        return Some(format!("changed file `{}` at line {first}", path.display()));
    }
    changes
        .changed_files
        .contains(path)
        .then(|| format!("changed file `{}`", path.display()))
}
fn line_context(path: &Path, line: usize, changes: &ChangeSet) -> Option<String> {
    let lines = changes.changed_lines.get(path)?;
    if !lines.contains(&line) {
        return None;
    }
    let (start, end) = contiguous_range(lines, line);
    Some(format!(
        "changed hunk `{}`:{}",
        path.display(),
        format_range(start, end)
    ))
}
fn range_context(path: &Path, range: (usize, usize), changes: &ChangeSet) -> Option<String> {
    let lines = changes.changed_lines.get(path);
    let contexts = lines
        .into_iter()
        .flat_map(|values| values.range(range.0..=range.1).copied())
        .collect::<Vec<_>>();
    if !contexts.is_empty() {
        return Some(format!("`{}`:{}", path.display(), format_ranges(&contexts)));
    }
    changes
        .changed_files
        .contains(path)
        .then(|| format!("`{}`", path.display()))
}
fn contiguous_range(lines: &BTreeSet<usize>, line: usize) -> (usize, usize) {
    let mut start = line;
    while start > 1 && lines.contains(&(start - 1)) {
        start -= 1;
    }
    let mut end = line;
    while lines.contains(&end.saturating_add(1)) {
        end = end.saturating_add(1);
    }
    (start, end)
}
fn format_ranges(lines: &[usize]) -> String {
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let start = lines[index];
        let mut end = start;
        while index + 1 < lines.len() && lines[index + 1] == end.saturating_add(1) {
            index += 1;
            end = lines[index];
        }
        ranges.push(format_range(start, end));
        index += 1;
    }
    ranges.join(",")
}
fn format_range(start: usize, end: usize) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}
