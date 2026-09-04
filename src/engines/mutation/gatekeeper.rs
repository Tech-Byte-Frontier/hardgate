use crate::config::MutationConfig;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// One mutation-report breach: kill rate, runner integrity, or timeout policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationViolation {
    pub report_file: PathBuf,
    pub metric: String,
    pub actual: f64,
    pub limit: f64,
    pub message: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationStats {
    pub killed: usize,
    pub survived: usize,
    pub timeout: usize,
    pub compile_error: usize,
    pub runner_error: usize,
    pub equivalent: usize,
    pub unviable: usize,
    pub total: usize,
}

impl MutationStats {
    pub fn score_percent(&self) -> f64 {
        let Some(viable) = self.viable_count() else {
            return 0.0;
        };
        if viable == 0 {
            return 0.0;
        }
        (self.killed as f64 / viable as f64) * 100.0
    }

    fn viable_count(&self) -> Option<usize> {
        self.killed.checked_add(self.survived)
    }

    fn add(&mut self, category: ReportCategory) -> Result<()> {
        self.total = checked_increment(self.total)?;
        match category {
            ReportCategory::Killed => self.killed = checked_increment(self.killed)?,
            ReportCategory::Survived => self.survived = checked_increment(self.survived)?,
            ReportCategory::Timeout => self.timeout = checked_increment(self.timeout)?,
            ReportCategory::CompileError => {
                self.compile_error = checked_increment(self.compile_error)?
            }
            ReportCategory::RunnerError => {
                self.runner_error = checked_increment(self.runner_error)?
            }
            ReportCategory::Equivalent => self.equivalent = checked_increment(self.equivalent)?,
            ReportCategory::Unviable => self.unviable = checked_increment(self.unviable)?,
        }
        Ok(())
    }

    fn merge(&mut self, other: &Self) -> Result<()> {
        self.killed = checked_add(self.killed, other.killed)?;
        self.survived = checked_add(self.survived, other.survived)?;
        self.timeout = checked_add(self.timeout, other.timeout)?;
        self.compile_error = checked_add(self.compile_error, other.compile_error)?;
        self.runner_error = checked_add(self.runner_error, other.runner_error)?;
        self.equivalent = checked_add(self.equivalent, other.equivalent)?;
        self.unviable = checked_add(self.unviable, other.unviable)?;
        self.total = checked_add(self.total, other.total)?;
        Ok(())
    }
}

pub struct MutationGatekeeper {
    config: MutationConfig,
}

impl MutationGatekeeper {
    pub fn new(config: &MutationConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    pub fn evaluate_report(&self, report_path: &Path) -> Result<Vec<MutationViolation>> {
        let content = fs::read_to_string(report_path)?;
        if content.trim().is_empty() {
            bail!("mutation report is empty: {}", report_path.display());
        }
        let json_val: Value = serde_json::from_str(&content)?;
        let stats = parse_mutation_json(&json_val)?;
        let score = stats.score_percent();
        let min_score = self.config.min_score.unwrap_or(85.0);
        let mut violations = Vec::new();

        if !matches!(stats.viable_count(), Some(1..)) || score < min_score {
            push_score_violation(
                &mut violations,
                report_path,
                ScoreSpec {
                    stats: &stats,
                    score,
                    min_score,
                },
            );
        }

        // Timeouts are always blocking evidence failures. `reject_timeouts` is
        // retained for configuration compatibility, but cannot weaken P0.
        for spec in [
            IntegritySpec {
                metric: "Mutation Timeouts",
                count: stats.timeout,
                message: "Mutation run had timed-out mutants.",
                recommendation: "Investigate and resolve infinite loops in test runs.",
            },
            IntegritySpec {
                metric: "Mutation Compile Errors",
                count: stats.compile_error,
                message: "Mutation report contains compile-error outcomes.",
                recommendation: "Repair mutants that do not compile or classify them in the source generator.",
            },
            IntegritySpec {
                metric: "Mutation Runner Errors",
                count: stats.runner_error,
                message: "Mutation report contains runner-error outcomes.",
                recommendation: "Repair the mutation command or test runner before trusting the score.",
            },
            IntegritySpec {
                metric: "Mutation Unviable Mutants",
                count: stats.unviable,
                message: "Mutation report contains unviable outcomes.",
                recommendation: "Remove or repair unviable mutants; do not let them mask missing coverage.",
            },
        ] {
            push_integrity_violation(&mut violations, report_path, spec);
        }

        Ok(violations)
    }
}

fn push_score_violation(
    violations: &mut Vec<MutationViolation>,
    report_path: &Path,
    spec: ScoreSpec<'_>,
) {
    violations.push(MutationViolation {
        report_file: report_path.to_path_buf(),
        metric: "Mutation Kill Rate".to_string(),
        actual: spec.score,
        limit: spec.min_score,
        message: format!(
            "Mutation testing score {:.1}% is below floor {:.1}% (Killed: {}, Survived: {}, Equivalent: {})",
            spec.score,
            spec.min_score,
            spec.stats.killed,
            spec.stats.survived,
            spec.stats.equivalent
        ),
        recommendation: "Write semantic assertions to catch mutant faults.".to_string(),
    });
}

struct ScoreSpec<'a> {
    stats: &'a MutationStats,
    score: f64,
    min_score: f64,
}

struct IntegritySpec<'a> {
    metric: &'a str,
    count: usize,
    message: &'a str,
    recommendation: &'a str,
}

fn push_integrity_violation(
    violations: &mut Vec<MutationViolation>,
    report_path: &Path,
    spec: IntegritySpec<'_>,
) {
    if spec.count == 0 {
        return;
    }
    violations.push(MutationViolation {
        report_file: report_path.to_path_buf(),
        metric: spec.metric.to_string(),
        actual: spec.count as f64,
        limit: 0.0,
        message: format!("{} Count: {}.", spec.message, spec.count),
        recommendation: spec.recommendation.to_string(),
    });
}

#[derive(Debug, Clone, Copy)]
enum ReportCategory {
    Killed,
    Survived,
    Timeout,
    CompileError,
    RunnerError,
    Equivalent,
    Unviable,
}

fn parse_mutation_json(val: &Value) -> Result<MutationStats> {
    let object = require_object(val, "mutation report root")?;
    if object.contains_key("files") {
        return parse_stryker_json(val);
    }
    if object.contains_key("outcomes") {
        return parse_cargo_mutants_json(val);
    }
    parse_generic_mutation_json(val)
}

fn parse_stryker_json(val: &Value) -> Result<MutationStats> {
    let root = require_object(val, "Stryker report root")?;
    let files = require_object_field(root, "files", "Stryker report")?;
    if files.is_empty() {
        bail!("Stryker report contains no files or mutants");
    }

    let mut stats = MutationStats::default();
    for (file_name, file_val) in files {
        let file_stats = parse_stryker_file(file_name, file_val)?;
        stats.merge(&file_stats)?;
    }
    validate_declared_total(root, &stats, "Stryker")?;
    ensure_nonempty(&stats, "Stryker")?;
    Ok(stats)
}

fn parse_cargo_mutants_json(val: &Value) -> Result<MutationStats> {
    let root = require_object(val, "cargo-mutants report root")?;
    let outcomes = require_array_field(root, "outcomes", "cargo-mutants report")?;
    if outcomes.is_empty() {
        bail!("cargo-mutants report contains no outcomes");
    }

    let stats = parse_status_entries(outcomes, "summary", "cargo-mutants")?;
    validate_declared_total(root, &stats, "cargo-mutants")?;
    ensure_nonempty(&stats, "cargo-mutants")?;
    Ok(stats)
}

fn parse_stryker_file(file_name: &str, file_val: &Value) -> Result<MutationStats> {
    let file_object = file_val.as_object().ok_or_else(|| {
        anyhow::anyhow!("Stryker report file entry `{file_name}` must be an object")
    })?;
    let mutants = require_array_field(file_object, "mutants", "Stryker file")?;
    let format = format!("Stryker file `{file_name}`");
    let stats = parse_status_entries(mutants, "status", &format)?;
    validate_declared_total(file_object, &stats, &format)?;
    Ok(stats)
}

fn parse_status_entries(entries: &[Value], field: &str, format: &str) -> Result<MutationStats> {
    let mut stats = MutationStats::default();
    for entry in entries {
        let object = entry
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("{format} outcome must be an object"))?;
        let status = object
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("{format} outcome has no `{field}` status"))?;
        stats.add(parse_status(status, format)?)?;
    }
    Ok(stats)
}

fn parse_generic_mutation_json(val: &Value) -> Result<MutationStats> {
    let object = require_object(val, "generic mutation report")?;
    if !GENERIC_COUNT_FIELDS
        .iter()
        .any(|key| object.contains_key(*key))
    {
        bail!("mutation report has no recognized outcome counts");
    }
    reject_unknown_generic_outcomes(object)?;

    let mut stats = MutationStats {
        killed: count_field(object, "killed")?,
        survived: count_field(object, "survived")?,
        timeout: count_field(object, "timeout")?,
        compile_error: count_field(object, "compile_error")?,
        runner_error: count_field(object, "runner_error")?,
        equivalent: count_field(object, "equivalent")?,
        unviable: count_field(object, "unviable")?,
        total: 0,
    };
    stats.total = checked_sum([
        stats.killed,
        stats.survived,
        stats.timeout,
        stats.compile_error,
        stats.runner_error,
        stats.equivalent,
        stats.unviable,
    ])?;
    validate_declared_total(object, &stats, "generic")?;
    ensure_nonempty(&stats, "generic")?;
    Ok(stats)
}

const GENERIC_COUNT_FIELDS: [&str; 7] = [
    "killed",
    "survived",
    "timeout",
    "compile_error",
    "runner_error",
    "equivalent",
    "unviable",
];

fn reject_unknown_generic_outcomes(object: &serde_json::Map<String, Value>) -> Result<()> {
    for (key, value) in object {
        if GENERIC_COUNT_FIELDS.contains(&key.as_str()) || key == "total" {
            continue;
        }
        let normalized = key.to_ascii_lowercase();
        let looks_like_status = ["status", "outcome", "result"]
            .iter()
            .any(|part| normalized.contains(part));
        if value.is_number() || looks_like_status {
            bail!("mutation report has unknown outcome field `{key}`");
        }
    }
    Ok(())
}

fn count_field(object: &serde_json::Map<String, Value>, key: &str) -> Result<usize> {
    object.get(key).map_or(Ok(0), |value| as_count(value, key))
}

fn as_count(value: &Value, key: &str) -> Result<usize> {
    let count = value
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("mutation report `{key}` must be a non-negative integer"))?;
    usize::try_from(count)
        .map_err(|_| anyhow::anyhow!("mutation report `{key}` is too large for this platform"))
}

fn require_object<'a>(
    value: &'a Value,
    format: &str,
) -> Result<&'a serde_json::Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("{format} must be a JSON object"))
}

fn require_object_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
    format: &str,
) -> Result<&'a serde_json::Map<String, Value>> {
    object
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("{format} `{key}` must be an object"))
}

fn require_array_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
    format: &str,
) -> Result<&'a [Value]> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow::anyhow!("{format} `{key}` must be an array"))
}

fn checked_increment(value: usize) -> Result<usize> {
    checked_add(value, 1)
}

fn checked_add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right)
        .ok_or_else(|| anyhow::anyhow!("mutation report outcome counts overflow usize"))
}

fn checked_sum(values: [usize; 7]) -> Result<usize> {
    values.into_iter().try_fold(0, checked_add)
}

fn validate_declared_total(
    object: &serde_json::Map<String, Value>,
    stats: &MutationStats,
    format: &str,
) -> Result<()> {
    let Some(total) = object.get("total") else {
        return Ok(());
    };
    let declared = as_count(total, "total")?;
    if declared != stats.total {
        bail!(
            "{format} total ({declared}) does not match outcome counts ({})",
            stats.total
        );
    }
    Ok(())
}

fn ensure_nonempty(stats: &MutationStats, format: &str) -> Result<()> {
    if stats.total == 0 {
        bail!("{format} mutation report contains no mutants");
    }
    Ok(())
}

fn parse_status(status: &str, format: &str) -> Result<ReportCategory> {
    let normalized = status
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' ', '/'], "_");
    classify_status(&normalized)
        .ok_or_else(|| anyhow::anyhow!("{format} mutation report has unknown status `{status}`"))
}

fn classify_status(status: &str) -> Option<ReportCategory> {
    if matches!(status, "killed" | "caught") {
        return Some(ReportCategory::Killed);
    }
    if matches!(status, "survived" | "missed") {
        return Some(ReportCategory::Survived);
    }
    if matches!(status, "timeout" | "timed_out") {
        return Some(ReportCategory::Timeout);
    }
    if matches!(
        status,
        "compileerror" | "compile_error" | "compilation_error"
    ) {
        return Some(ReportCategory::CompileError);
    }
    if matches!(
        status,
        "runtimeerror" | "runtime_error" | "runnererror" | "runner_error" | "error"
    ) {
        return Some(ReportCategory::RunnerError);
    }
    if status == "equivalent" {
        return Some(ReportCategory::Equivalent);
    }
    if matches!(
        status,
        "unviable"
            | "no_coverage"
            | "nocoverage"
            | "not_covered"
            | "notcovered"
            | "ignored"
            | "pending"
            | "not_run"
            | "notrun"
    ) {
        return Some(ReportCategory::Unviable);
    }
    None
}
