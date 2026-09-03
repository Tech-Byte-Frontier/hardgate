mod agent;
mod compact;
mod summary;
mod terminal;

use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, CoverageViolation, DeadCodeViolation,
    InvariantViolation, MutationViolation, OrchestrationViolation, SuppressionViolation,
};
use colored::*;
use serde::{Deserialize, Serialize};

pub use summary::{GateSummary, TopFileEntry};

/// Aggregated result of a gate run: every violation by category plus the
/// counts needed for terminal, agent, compact, summary, and JSON rendering.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GateReport {
    pub gate_name: String,
    pub files_scanned: usize,
    pub functions_analyzed: usize,
    pub duration_ms: u128,
    pub passed: bool,
    #[serde(default)]
    pub advisories: Vec<String>,
    pub budget_violations: Vec<BudgetViolation>,
    pub suppression_violations: Vec<SuppressionViolation>,
    pub complexity_violations: Vec<ComplexityViolation>,
    pub invariant_violations: Vec<InvariantViolation>,
    pub clone_violations: Vec<CloneViolation>,
    pub coverage_violations: Vec<CoverageViolation>,
    pub mutation_violations: Vec<MutationViolation>,
    pub dead_code_violations: Vec<DeadCodeViolation>,
    pub orchestration_violations: Vec<OrchestrationViolation>,
}

impl GateReport {
    /// Create an empty report for `gate_name`; call [`GateReport::finalize`]
    /// once analysis is done to freeze counts and the pass/fail verdict.
    pub fn new(gate_name: String) -> Self {
        Self {
            gate_name,
            passed: true,
            ..Default::default()
        }
    }

    /// Total violations across every category.
    pub fn total_violations(&self) -> usize {
        [
            self.budget_violations.len(),
            self.suppression_violations.len(),
            self.complexity_violations.len(),
            self.invariant_violations.len(),
            self.clone_violations.len(),
            self.coverage_violations.len(),
            self.mutation_violations.len(),
            self.dead_code_violations.len(),
            self.orchestration_violations.len(),
        ]
        .iter()
        .sum()
    }

    /// Freeze scan counts and derive `passed` (true only with zero violations).
    pub fn finalize(&mut self, files_scanned: usize, functions_analyzed: usize, duration_ms: u128) {
        self.files_scanned = files_scanned;
        self.functions_analyzed = functions_analyzed;
        self.duration_ms = duration_ms;
        self.passed = self.total_violations() == 0;
    }
}

pub(crate) fn status_label(passed: bool, total_errors: usize) -> ColoredString {
    if passed {
        "pass".bold().green()
    } else {
        format!("fail ({total_errors} errors)").bold().red()
    }
}

/// Shared `hardgate [gate] status` banner so terminal, compact, and summary
/// renderers cannot drift (or clone) apart.
pub(crate) fn push_gate_header(out: &mut String, gate_name: &str, status: ColoredString) {
    out.push_str(&format!(
        "\n{} {} {}\n{}\n\n",
        "hardgate".bold(),
        format!("[{gate_name}]").bold(),
        status,
        "-".repeat(70).dimmed()
    ));
}

/// Single compact diagnostic line: red title plus `-->` target reference.
pub(crate) fn push_compact_entry(out: &mut String, title: String, target: String) {
    out.push_str(&format!("{}\n  --> {}\n", title.bold().red(), target));
}
