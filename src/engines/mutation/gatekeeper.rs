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
        let viable = self.killed + self.survived;
        if viable == 0 {
            0.0
        } else {
            (self.killed as f64 / viable as f64) * 100.0
        }
    }

    fn add(&mut self, category: ReportCategory) {
        self.total += 1;
        match category {
            ReportCategory::Killed => self.killed += 1,
            ReportCategory::Survived => self.survived += 1,
            ReportCategory::Timeout => self.timeout += 1,
            ReportCategory::CompileError => self.compile_error += 1,
            ReportCategory::RunnerError => self.runner_error += 1,
            ReportCategory::Equivalent => self.equivalent += 1,
            ReportCategory::Unviable => self.unviable += 1,
        }
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

        if score < min_score {
            violations.push(MutationViolation {
                report_file: report_path.to_path_buf(),
                metric: "Mutation Kill Rate".to_string(),
                actual: score,
                limit: min_score,
                message: format!(
                    "Mutation testing score {:.1}% is below floor {:.1}% (Killed: {}, Survived: {})",
                    score, min_score, stats.killed, stats.survived
                ),
                recommendation: "Write semantic assertions to catch mutant faults.".to_string(),
            });
        }

        if self.config.reject_timeouts && stats.timeout > 0 {
            violations.push(MutationViolation {
                report_file: report_path.to_path_buf(),
                metric: "Mutation Timeouts".to_string(),
                actual: stats.timeout as f64,
                limit: 0.0,
                message: format!("Mutation run had {} timed-out mutants.", stats.timeout),
                recommendation: "Investigate and resolve infinite loops in test runs.".to_string(),
            });
        }

        push_integrity_violation(
            &mut violations,
            report_path,
            "Mutation Compile Errors",
            stats.compile_error,
            "Mutation report contains compile-error outcomes.",
            "Repair mutants that do not compile or classify them in the source generator.",
        );
        push_integrity_violation(
            &mut violations,
            report_path,
            "Mutation Runner Errors",
            stats.runner_error,
            "Mutation report contains runner-error outcomes.",
            "Repair the mutation command or test runner before trusting the score.",
        );
        push_integrity_violation(
            &mut violations,
            report_path,
            "Mutation Unviable Mutants",
            stats.unviable,
            "Mutation report contains unviable outcomes.",
            "Remove or repair unviable mutants; do not let them mask missing coverage.",
        );

        Ok(violations)
    }
}

fn push_integrity_violation(
    violations: &mut Vec<MutationViolation>,
    report_path: &Path,
    metric: &str,
    count: usize,
    message: &str,
    recommendation: &str,
) {
    if count == 0 {
        return;
    }
    violations.push(MutationViolation {
        report_file: report_path.to_path_buf(),
        metric: metric.to_string(),
        actual: count as f64,
        limit: 0.0,
        message: format!("{message} Count: {count}."),
        recommendation: recommendation.to_string(),
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
    let object = val
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("mutation report root must be a JSON object"))?;

    if object.contains_key("files") {
        return parse_stryker_json(val);
    }
    if object.contains_key("outcomes") {
        return parse_cargo_mutants_json(val);
    }
    parse_generic_mutation_json(val)
}

fn parse_stryker_json(val: &Value) -> Result<MutationStats> {
    let files = val
        .get("files")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("Stryker report `files` must be an object"))?;
    if files.is_empty() {
        bail!("Stryker report contains no files or mutants");
    }

    let mut stats = MutationStats::default();
    for (file_name, file_val) in files {
        let file_object = file_val.as_object().ok_or_else(|| {
            anyhow::anyhow!("Stryker report file entry `{file_name}` must be an object")
        })?;
        let mutants = file_object
            .get("mutants")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("Stryker file `{file_name}` has no mutants array"))?;
        for mutant in mutants {
            let mutant_object = mutant.as_object().ok_or_else(|| {
                anyhow::anyhow!("Stryker mutant in `{file_name}` must be an object")
            })?;
            let status = mutant_object
                .get("status")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("Stryker mutant in `{file_name}` has no status"))?;
            stats.add(parse_status(status, "Stryker")?);
        }
    }
    ensure_nonempty(&stats, "Stryker")?;
    Ok(stats)
}

fn parse_cargo_mutants_json(val: &Value) -> Result<MutationStats> {
    let outcomes = val
        .get("outcomes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("cargo-mutants report `outcomes` must be an array"))?;
    if outcomes.is_empty() {
        bail!("cargo-mutants report contains no outcomes");
    }

    let mut stats = MutationStats::default();
    for outcome in outcomes {
        let outcome_object = outcome
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("cargo-mutants outcome must be an object"))?;
        let summary = outcome_object
            .get("summary")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("cargo-mutants outcome has no summary"))?;
        stats.add(parse_status(summary, "cargo-mutants")?);
    }
    ensure_nonempty(&stats, "cargo-mutants")?;
    Ok(stats)
}

fn parse_generic_mutation_json(val: &Value) -> Result<MutationStats> {
    let object = val
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("generic mutation report must be an object"))?;
    let known = [
        "killed",
        "survived",
        "timeout",
        "compile_error",
        "runner_error",
        "equivalent",
        "unviable",
    ];
    if !known.iter().any(|key| object.contains_key(*key)) {
        bail!("mutation report has no recognized outcome counts");
    }

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
    let computed_total = [
        stats.killed,
        stats.survived,
        stats.timeout,
        stats.compile_error,
        stats.runner_error,
        stats.equivalent,
        stats.unviable,
    ]
    .into_iter()
    .try_fold(0usize, usize::checked_add)
    .ok_or_else(|| anyhow::anyhow!("mutation report outcome counts overflow usize"))?;
    if let Some(total) = object.get("total") {
        let declared = as_count(total, "total")?;
        if declared != computed_total {
            bail!(
                "mutation report total ({declared}) does not match outcome counts ({computed_total})"
            );
        }
    }
    stats.total = computed_total;
    ensure_nonempty(&stats, "generic")?;
    Ok(stats)
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
    let category = match normalized.as_str() {
        "killed" | "caught" => ReportCategory::Killed,
        "survived" | "missed" => ReportCategory::Survived,
        "timeout" | "timed_out" => ReportCategory::Timeout,
        "compileerror" | "compile_error" | "compilation_error" => ReportCategory::CompileError,
        "runtimeerror" | "runtime_error" | "runnererror" | "runner_error" | "error" => {
            ReportCategory::RunnerError
        }
        "equivalent" => ReportCategory::Equivalent,
        "unviable" | "no_coverage" | "nocoverage" | "not_covered" | "notcovered" | "ignored"
        | "pending" | "not_run" | "notrun" => ReportCategory::Unviable,
        _ => bail!("{format} mutation report has unknown status `{status}`"),
    };
    Ok(category)
}
