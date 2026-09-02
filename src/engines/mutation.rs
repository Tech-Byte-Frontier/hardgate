use crate::config::MutationConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationViolation {
    pub report_file: PathBuf,
    pub metric: String,
    pub actual: f64,
    pub limit: f64,
    pub message: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Default)]
pub struct MutationStats {
    pub killed: usize,
    pub survived: usize,
    pub timeout: usize,
    pub unviable: usize,
    pub total: usize,
}

impl MutationStats {
    pub fn score_percent(&self) -> f64 {
        let viable = self.killed + self.survived;
        if viable == 0 {
            100.0
        } else {
            (self.killed as f64 / viable as f64) * 100.0
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

    pub fn evaluate_report(&self, report_path: &Path) -> anyhow::Result<Vec<MutationViolation>> {
        let mut violations = Vec::new();
        let content = fs::read_to_string(report_path)?;
        let json_val: serde_json::Value = serde_json::from_str(&content)?;

        let stats = parse_mutation_json(&json_val);
        let score = stats.score_percent();
        let min_score = self.config.min_score.unwrap_or(85.0);

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

        Ok(violations)
    }
}

fn parse_mutation_json(val: &serde_json::Value) -> MutationStats {
    if let Some(stats) = parse_stryker_json(val) {
        return stats;
    }
    if let Some(stats) = parse_cargo_mutants_json(val) {
        return stats;
    }
    parse_generic_mutation_json(val)
}

fn parse_stryker_json(val: &serde_json::Value) -> Option<MutationStats> {
    let files = val.get("files")?.as_object()?;
    let mut stats = MutationStats::default();

    for file_val in files.values() {
        if let Some(mutants) = file_val.get("mutants").and_then(|m| m.as_array()) {
            accumulate_stryker_mutants(mutants, &mut stats);
        }
    }

    Some(stats)
}

fn accumulate_stryker_mutants(mutants: &[serde_json::Value], stats: &mut MutationStats) {
    for m in mutants {
        stats.total += 1;
        match m.get("status").and_then(|s| s.as_str()) {
            Some("Killed") => stats.killed += 1,
            Some("Survived") => stats.survived += 1,
            Some("Timeout") => stats.timeout += 1,
            _ => stats.unviable += 1,
        }
    }
}

fn parse_cargo_mutants_json(val: &serde_json::Value) -> Option<MutationStats> {
    let mutants = val.get("outcomes")?.as_array()?;
    let mut stats = MutationStats::default();

    for m in mutants {
        stats.total += 1;
        let summary = m.get("summary").and_then(|s| s.as_str()).unwrap_or("");
        if summary == "caught" {
            stats.killed += 1;
        } else if summary == "missed" {
            stats.survived += 1;
        } else if summary == "timeout" {
            stats.timeout += 1;
        } else {
            stats.unviable += 1;
        }
    }

    Some(stats)
}

fn parse_generic_mutation_json(val: &serde_json::Value) -> MutationStats {
    let mut stats = MutationStats::default();
    if let Some(k) = val.get("killed").and_then(|v| v.as_u64()) {
        stats.killed = k as usize;
    }
    if let Some(s) = val.get("survived").and_then(|v| v.as_u64()) {
        stats.survived = s as usize;
    }
    if let Some(t) = val.get("timeout").and_then(|v| v.as_u64()) {
        stats.timeout = t as usize;
    }
    stats.total = stats.killed + stats.survived + stats.timeout + stats.unviable;
    stats
}
