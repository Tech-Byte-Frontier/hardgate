mod agent;

use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityContribution, ComplexityViolation,
    CoverageViolation, DeadCodeViolation, InvariantViolation, MutationViolation,
    OrchestrationViolation, SuppressionViolation,
};
use colored::*;
use serde::{Deserialize, Serialize};

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
    pub fn new(gate_name: String) -> Self {
        Self {
            gate_name,
            passed: true,
            ..Default::default()
        }
    }

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

    pub fn finalize(&mut self, files_scanned: usize, functions_analyzed: usize, duration_ms: u128) {
        self.files_scanned = files_scanned;
        self.functions_analyzed = functions_analyzed;
        self.duration_ms = duration_ms;
        self.passed = self.total_violations() == 0;
    }

    pub fn render_terminal(&self) -> String {
        let mut out = String::new();
        self.render_terminal_header(&mut out);
        self.render_advisories_terminal(&mut out);
        self.render_suppressions_terminal(&mut out);
        self.render_complexity_terminal(&mut out);
        self.render_budgets_terminal(&mut out);
        self.render_invariants_terminal(&mut out);
        self.render_clones_terminal(&mut out);
        self.render_coverage_terminal(&mut out);
        self.render_mutation_terminal(&mut out);
        self.render_dead_code_terminal(&mut out);
        self.render_orchestration_terminal(&mut out);
        self.render_terminal_summary(&mut out);
        out
    }

    fn render_terminal_header(&self, out: &mut String) {
        let status = if self.passed {
            "[PASSED]".bold().green()
        } else {
            "[FAILED]".bold().red()
        };
        out.push_str(&format!(
            "\n{} {} {}\n{}\n\n",
            "🛡️ ".bold(),
            format!("Hardgate Gate [{}]", self.gate_name).bold().cyan(),
            status,
            "─".repeat(70).dimmed()
        ));
    }

    fn render_advisories_terminal(&self, out: &mut String) {
        if self.advisories.is_empty() {
            return;
        }
        for advisory in &self.advisories {
            out.push_str(&format!(
                "{}  {}: {}\n",
                "⚠️".yellow(),
                "Advisory".yellow().bold(),
                advisory
            ));
        }
        out.push('\n');
    }

    fn render_suppressions_terminal(&self, out: &mut String) {
        if self.suppression_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{} {}\n",
            "🚫".red(),
            format!(
                "Anti-Gaming Violations ({})",
                self.suppression_violations.len()
            )
            .bold()
            .red()
        ));
        for v in &self.suppression_violations {
            out.push_str(&format!(
                "   • {}:{} - Forbidden: {}\n     {}\n",
                v.file.display().to_string().bold(),
                v.line_number.to_string().yellow(),
                v.token.bold().red(),
                v.line_content.dimmed()
            ));
        }
        out.push('\n');
    }

    fn render_complexity_terminal(&self, out: &mut String) {
        if self.complexity_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{} {}\n",
            "⚡".yellow(),
            format!(
                "Complexity Violations ({})",
                self.complexity_violations.len()
            )
            .bold()
            .yellow()
        ));
        for v in &self.complexity_violations {
            out.push_str(&format!(
                "   • {}:{} [{}] - {}: actual {:.0}, budget {:.0}\n",
                v.file.display().to_string().bold(),
                v.line_number.to_string().yellow(),
                v.function_name.cyan(),
                v.metric,
                v.actual,
                v.limit
            ));
            append_terminal_contributors(&v.breakdown, out);
            out.push_str(&format!("     Hint: {}\n", v.recommendation.dimmed()));
        }
        out.push('\n');
    }

    fn render_budgets_terminal(&self, out: &mut String) {
        if self.budget_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{} {}\n",
            "📦".yellow(),
            format!("File Budget Violations ({})", self.budget_violations.len())
                .bold()
                .yellow()
        ));
        for v in &self.budget_violations {
            out.push_str(&format!(
                "   • {} - {}: actual {}, limit {}\n",
                v.file.display().to_string().bold(),
                v.metric,
                v.actual.to_string().red(),
                v.limit.to_string().green()
            ));
        }
        out.push('\n');
    }

    fn render_invariants_terminal(&self, out: &mut String) {
        if self.invariant_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{} {}\n",
            "🏛️".yellow(),
            format!(
                "Architectural Invariant Violations ({})",
                self.invariant_violations.len()
            )
            .bold()
            .yellow()
        ));
        for v in &self.invariant_violations {
            out.push_str(&format!(
                "   • {}:{} [{}] - {}\n     Hint: Target `{}` violated boundary `{}`.\n",
                v.file.display().to_string().bold(),
                v.line_number.to_string().yellow(),
                v.rule_name.bold().red(),
                v.message,
                v.offending_target.bold(),
                v.violation_type
            ));
        }
        out.push('\n');
    }

    fn render_clones_terminal(&self, out: &mut String) {
        if self.clone_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{} {}\n",
            "👥".yellow(),
            format!("Clone Violations ({})", self.clone_violations.len())
                .bold()
                .yellow()
        ));
        for v in &self.clone_violations {
            out.push_str(&format!("   • `{}:{}-{}` matches `{}:{}-{}` ({} lines, ~{} tokens)\n     Hint: Extract duplicated logic in `{}` and `{}` into a shared helper.\n", v.file_a.display(), v.lines_a.0, v.lines_a.1, v.file_b.display(), v.lines_b.0, v.lines_b.1, v.lines, v.tokens, v.file_a.display(), v.file_b.display()));
        }
        out.push('\n');
    }

    fn render_coverage_terminal(&self, out: &mut String) {
        if self.coverage_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{} {}\n",
            "🎯".yellow(),
            format!(
                "Coverage / CRAP Violations ({})",
                self.coverage_violations.len()
            )
            .bold()
            .yellow()
        ));
        for v in &self.coverage_violations {
            let func_info = v
                .function_name
                .as_ref()
                .map(|f| format!(" in function `{}`", f))
                .unwrap_or_default();
            out.push_str(&format!(
                "   • {}{} - {}: actual {:.1}, required {:.1}\n     Hint: {}\n",
                v.file.display().to_string().bold(),
                func_info.cyan(),
                v.metric,
                v.actual,
                v.limit,
                v.recommendation.dimmed()
            ));
        }
        out.push('\n');
    }

    fn render_mutation_terminal(&self, out: &mut String) {
        if self.mutation_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{} {}\n",
            "🧬".yellow(),
            format!(
                "Mutation Testing Violations ({})",
                self.mutation_violations.len()
            )
            .bold()
            .yellow()
        ));
        for v in &self.mutation_violations {
            out.push_str(&format!(
                "   • {} - {}: actual {:.1}%, required {:.1}%\n     Hint: {}\n",
                v.report_file.display().to_string().bold(),
                v.metric,
                v.actual,
                v.limit,
                v.recommendation.dimmed()
            ));
        }
        out.push('\n');
    }

    fn render_dead_code_terminal(&self, out: &mut String) {
        if self.dead_code_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{} {}\n",
            "🧹".yellow(),
            format!(
                "Dead Code & Unused Export Violations ({})",
                self.dead_code_violations.len()
            )
            .bold()
            .yellow()
        ));
        for v in &self.dead_code_violations {
            let line_str = v.line_number.map(|l| format!(":{}", l)).unwrap_or_default();
            out.push_str(&format!(
                "   • {}{} [{}] - {}\n     Hint: {}\n",
                v.file.display().to_string().bold(),
                line_str.yellow(),
                v.violation_type.cyan(),
                v.message,
                v.recommendation.dimmed()
            ));
        }
        out.push('\n');
    }

    fn render_orchestration_terminal(&self, out: &mut String) {
        if self.orchestration_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{} {}\n",
            "🛠️ ".red(),
            format!(
                "Orchestration Tool Failures ({})",
                self.orchestration_violations.len()
            )
            .bold()
            .red()
        ));
        for v in &self.orchestration_violations {
            out.push_str(&format!(
                "   • [{}] `{}` failed (exit code: {:?})\n     Output: {}\n     Hint: {}\n",
                v.step.bold().red(),
                v.command.cyan(),
                v.exit_code,
                v.output.dimmed(),
                v.recommendation.dimmed()
            ));
        }
        out.push('\n');
    }

    fn render_terminal_summary(&self, out: &mut String) {
        let verdict = if self.passed {
            "PASS (All gates satisfied)".bold().green()
        } else {
            format!("FAIL ({} violations detected)", self.total_violations())
                .bold()
                .red()
        };
        out.push_str(&format!(
            "{}\nSummary: {} files scanned, {} functions analyzed in {}ms.\nVerdict: {}\n",
            "─".repeat(70).dimmed(),
            self.files_scanned,
            self.functions_analyzed,
            self.duration_ms,
            verdict
        ));
    }

    pub fn render_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

struct ContributorFormat<'a> {
    header: &'a str,
    prefix: &'a str,
    dim: bool,
}

fn append_terminal_contributors(breakdown: &[ComplexityContribution], out: &mut String) {
    append_contributors_formatted(
        breakdown,
        ContributorFormat {
            header: "     Key AST contributors:\n",
            prefix: "       • L",
            dim: true,
        },
        out,
    );
}

fn append_contributors_formatted(
    breakdown: &[ComplexityContribution],
    fmt: ContributorFormat,
    out: &mut String,
) {
    if breakdown.is_empty() {
        return;
    }
    out.push_str(fmt.header);
    for b in breakdown {
        let desc = if fmt.dim {
            b.description.dimmed().to_string()
        } else {
            b.description.clone()
        };
        out.push_str(&format!(
            "{}{}: +{} for {}\n",
            fmt.prefix, b.line, b.score, desc
        ));
    }
}
