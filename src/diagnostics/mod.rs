mod agent;
mod agent_details;
mod compact;
pub mod display;
mod display_order;
pub mod execution;
pub(crate) mod execution_observations;
pub mod filter;
mod machine;
mod review;
pub use review::FunctionReview;
pub mod rules;
mod summary;
mod terminal;
mod triage_context;

use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, CoverageViolation, InvariantViolation,
    MutationViolation, OrchestrationViolation, SuppressionViolation,
};
use colored::*;
use serde::{Deserialize, Serialize};

pub use summary::{GateSummary, TopFileEntry};

/// Aggregated result of a gate run: every violation by category plus the
/// counts needed for terminal, agent, compact, summary, and JSON rendering.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GateReport {
    #[serde(default)]
    pub execution: Option<execution::ExecutionPlan>,
    #[serde(skip)]
    pub(crate) engine_observations:
        std::collections::BTreeMap<execution::EngineId, execution::EngineState>,
    #[serde(skip)]
    pub(crate) engine_reasons: std::collections::BTreeMap<execution::EngineId, String>,
    #[serde(skip)]
    pub display: display::DisplayOptions,
    #[serde(skip)]
    pub(crate) source_text: std::collections::BTreeMap<std::path::PathBuf, std::sync::Arc<str>>,
    #[serde(skip)]
    pub(crate) saved_excerpts: Vec<display::SourceExcerpt>,
    #[serde(skip)]
    pub(crate) saved_failures: Vec<rules::RuleDiagnostic>,
    #[serde(skip)]
    pub(crate) saved_summary: Option<GateSummary>,
    #[serde(skip)]
    pub(crate) saved_outcome: Option<crate::commands::CommandOutcome>,
    pub gate_name: String,
    pub files_scanned: usize,
    pub functions_analyzed: usize,
    pub duration_ms: u128,
    pub passed: bool,
    /// Per-function observations emitted by scan, including passing functions.
    #[serde(default)]
    pub functions: Vec<crate::engines::FunctionMetrics>,
    #[serde(default)]
    pub file_sizes: Vec<FileSizeMetrics>,
    #[serde(default)]
    pub advisories: Vec<String>,
    pub budget_violations: Vec<BudgetViolation>,
    pub suppression_violations: Vec<SuppressionViolation>,
    pub complexity_violations: Vec<ComplexityViolation>,
    pub invariant_violations: Vec<InvariantViolation>,
    pub clone_violations: Vec<CloneViolation>,
    pub coverage_violations: Vec<CoverageViolation>,
    pub mutation_violations: Vec<MutationViolation>,
    pub orchestration_violations: Vec<OrchestrationViolation>,
    #[serde(default)]
    pub tool_diagnostics: Vec<crate::engines::cargo_diagnostics::ToolDiagnostic>,
}

/// Syntax-aware size observations for a selected file and policy role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSizeMetrics {
    pub file: std::path::PathBuf,
    pub role: crate::discovery::FileRole,
    pub size: crate::engines::complexity::SizeBreakdown,
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
            self.orchestration_violations.len(),
            self.tool_findings_count(),
        ]
        .iter()
        .sum()
    }

    /// Count of genuine code findings (complexity, budgets, suppressions, invariants, clones, coverage, mutation).
    pub fn code_findings_count(&self) -> usize {
        self.budget_violations.len()
            + self.suppression_violations.len()
            + self.complexity_violations.len()
            + self.invariant_violations.len()
            + self.clone_violations.len()
            + self.coverage_violations.len()
            + self.mutation_violations.len()
            + self.tool_findings_count()
    }

    pub fn tool_findings_count(&self) -> usize {
        self.tool_diagnostics
            .iter()
            .filter(|finding| finding.blocking)
            .count()
    }

    pub(crate) fn render_specialist_findings(&self, out: &mut String) {
        for finding in &self.tool_diagnostics {
            out.push_str(&format!(
                "{}[{}]: {}\n  --> {}:{}:{}\n",
                finding.level,
                finding.rule,
                finding.message,
                finding.file.display(),
                finding.line,
                finding.column
            ));
        }
    }

    pub(crate) fn failure_diagnostics(&self) -> Vec<rules::RuleDiagnostic> {
        let mut failures = rules::diagnostics(self)
            .into_iter()
            .filter(|finding| {
                finding.category == "orchestration"
                    || finding.rule_id.starts_with("HG-COVERAGE-MISSING-")
            })
            .collect::<Vec<_>>();
        for failure in &self.saved_failures {
            if !failures.contains(failure) {
                failures.push(failure.clone());
            }
        }
        failures
    }

    /// Count of analysis blockers and tool/evidence failures (orchestration / report failures).
    pub fn analysis_blockers_count(&self) -> usize {
        self.orchestration_violations.len()
    }

    /// Freeze scan counts and derive `passed` (true only with zero violations).
    pub fn finalize(&mut self, files_scanned: usize, functions_analyzed: usize, duration_ms: u128) {
        self.files_scanned = files_scanned;
        self.functions_analyzed = functions_analyzed;
        self.duration_ms = duration_ms;
        self.passed = self.total_violations() == 0;
        self.finalize_execution();
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
