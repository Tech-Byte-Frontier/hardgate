#[path = "coverage/evaluator.rs"]
mod evaluator;
#[path = "coverage/lcov.rs"]
mod lcov;
#[path = "coverage/paths.rs"]
mod paths;
#[path = "coverage/scoring.rs"]
mod scoring;
#[path = "coverage/source_lines.rs"]
mod source_lines;

use crate::config::CoverageConfig;
use crate::engines::complexity::FunctionMetrics;
pub use lcov::FileCoverage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub use scoring::CoverageEvaluationScope;

pub(crate) use paths::normalized_repository_key;
pub(crate) use source_lines::retain_code_lines;

/// One coverage or CRAP breach for a file or function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageViolation {
    pub file: PathBuf,
    pub function_name: Option<String>,
    pub metric: String,
    pub actual: f64,
    pub limit: f64,
    pub message: String,
    pub recommendation: String,
}

pub struct CoverageScorer {
    config: CoverageConfig,
}

fn count_overflow_violation(file: &Path) -> CoverageViolation {
    CoverageViolation {
        file: file.to_path_buf(),
        function_name: None,
        metric: "Coverage Count Overflow".to_string(),
        actual: 0.0,
        limit: 0.0,
        message: "Coverage counters exceeded the representable range".to_string(),
        recommendation: "Regenerate a bounded LCOV report with valid counters.".to_string(),
    }
}

pub(crate) fn calc_ratio(hit: usize, found: usize) -> f64 {
    if found == 0 {
        0.0
    } else {
        hit as f64 / found as f64
    }
}

pub(crate) fn calc_pct(hit: usize, found: usize) -> f64 {
    calc_ratio(hit, found) * 100.0
}

impl CoverageScorer {
    pub fn new(config: &CoverageConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    pub fn parse_lcov(&self, report_path: &Path) -> anyhow::Result<HashMap<PathBuf, FileCoverage>> {
        lcov::parse_report(
            report_path,
            self.config.min_function_percent.is_some(),
            self.config.min_branch_percent.is_some(),
        )
    }

    /// Evaluate all report records. This compatibility API treats every
    /// uniquely indexed report record as a production candidate.
    pub fn evaluate(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        functions: &[FunctionMetrics],
        root: &Path,
    ) -> Vec<CoverageViolation> {
        self.evaluate_internal(
            coverage_map,
            functions,
            CoverageEvaluationScope {
                root,
                source_files: None,
            },
        )
    }

    /// Evaluate only records corresponding to the current Source inventory.
    pub fn evaluate_for_sources(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        functions: &[FunctionMetrics],
        scope: CoverageEvaluationScope<'_>,
    ) -> Vec<CoverageViolation> {
        self.evaluate_internal(coverage_map, functions, scope)
    }

    pub fn evaluate_diff_coverage(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        changed_lines: &BTreeMap<PathBuf, BTreeSet<usize>>,
    ) -> Vec<CoverageViolation> {
        self.evaluate_diff_coverage_legacy(coverage_map, changed_lines)
    }

    /// Strict diff scoring: every supplied line is code-bearing and must have
    /// a DA entry. Missing entries and zero-hit entries are both blocking.
    pub fn evaluate_diff_coverage_strict(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        changed_lines: &BTreeMap<PathBuf, BTreeSet<usize>>,
        root: &Path,
    ) -> Vec<CoverageViolation> {
        self.evaluate_diff_coverage_strict_impl(coverage_map, changed_lines, root)
    }
}
