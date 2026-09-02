use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, CoverageViolation, InvariantViolation,
    MutationViolation, SuppressionViolation,
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
    pub budget_violations: Vec<BudgetViolation>,
    pub suppression_violations: Vec<SuppressionViolation>,
    pub complexity_violations: Vec<ComplexityViolation>,
    pub invariant_violations: Vec<InvariantViolation>,
    pub clone_violations: Vec<CloneViolation>,
    pub coverage_violations: Vec<CoverageViolation>,
    pub mutation_violations: Vec<MutationViolation>,
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
        self.budget_violations.len()
            + self.suppression_violations.len()
            + self.complexity_violations.len()
            + self.invariant_violations.len()
            + self.clone_violations.len()
            + self.coverage_violations.len()
            + self.mutation_violations.len()
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
        self.render_suppressions_terminal(&mut out);
        self.render_complexity_terminal(&mut out);
        self.render_budgets_terminal(&mut out);
        self.render_invariants_terminal(&mut out);
        self.render_clones_terminal(&mut out);
        self.render_coverage_terminal(&mut out);
        self.render_mutation_terminal(&mut out);
        self.render_terminal_summary(&mut out);
        out
    }

    fn render_terminal_header(&self, out: &mut String) {
        let status = if self.passed { "[PASSED]".bold().green() } else { "[FAILED]".bold().red() };
        out.push_str(&format!("\n{} {} {}\n", "🛡️ ".bold(), format!("Hardgate Gate [{}]", self.gate_name).bold().cyan(), status));
        out.push_str(&format!("{}\n\n", "─".repeat(70).dimmed()));
    }

    fn render_suppressions_terminal(&self, out: &mut String) {
        if self.suppression_violations.is_empty() { return; }
        out.push_str(&format!("{} {}\n", "🚫".red(), format!("Anti-Gaming Violations ({})", self.suppression_violations.len()).bold().red()));
        for v in &self.suppression_violations {
            out.push_str(&format!("   • {}:{} - Forbidden: {}\n     {}\n", v.file.display().to_string().bold(), v.line_number.to_string().yellow(), v.token.bold().red(), v.line_content.dimmed()));
        }
        out.push('\n');
    }

    fn render_complexity_terminal(&self, out: &mut String) {
        if self.complexity_violations.is_empty() { return; }
        out.push_str(&format!("{} {}\n", "⚡".yellow(), format!("Complexity Violations ({})", self.complexity_violations.len()).bold().yellow()));
        for v in &self.complexity_violations {
            out.push_str(&format!("   • {}:{} [{}] - {}: actual {:.0}, budget {:.0}\n     Hint: {}\n", v.file.display().to_string().bold(), v.line_number.to_string().yellow(), v.function_name.cyan(), v.metric, v.actual, v.limit, v.recommendation.dimmed()));
        }
        out.push('\n');
    }

    fn render_budgets_terminal(&self, out: &mut String) {
        if self.budget_violations.is_empty() { return; }
        out.push_str(&format!("{} {}\n", "📦".yellow(), format!("File Budget Violations ({})", self.budget_violations.len()).bold().yellow()));
        for v in &self.budget_violations {
            out.push_str(&format!("   • {} - {}: actual {}, limit {}\n", v.file.display().to_string().bold(), v.metric, v.actual.to_string().red(), v.limit.to_string().green()));
        }
        out.push('\n');
    }

    fn render_invariants_terminal(&self, out: &mut String) {
        if self.invariant_violations.is_empty() { return; }
        out.push_str(&format!("{} {}\n", "🏛️ ".magenta(), format!("Boundary Violations ({})", self.invariant_violations.len()).bold().magenta()));
        for v in &self.invariant_violations {
            out.push_str(&format!("   • {}:{} [{}] - {}: {}\n     {}\n", v.file.display().to_string().bold(), v.line_number.to_string().yellow(), v.rule_name.cyan(), v.violation_type, v.offending_target.bold().red(), v.message.dimmed()));
        }
        out.push('\n');
    }

    fn render_clones_terminal(&self, out: &mut String) {
        if self.clone_violations.is_empty() { return; }
        out.push_str(&format!("{} {}\n", "👥".cyan(), format!("Clone Violations ({})", self.clone_violations.len()).bold().cyan()));
        for v in &self.clone_violations {
            out.push_str(&format!("   • `{}:{}-{}` matches `{}:{}-{}` ({} lines, ~{} tokens)\n     Hint: {}\n", v.file_a.display(), v.lines_a.0, v.lines_a.1, v.file_b.display(), v.lines_b.0, v.lines_b.1, v.lines, v.tokens, v.recommendation.dimmed()));
        }
        out.push('\n');
    }

    fn render_coverage_terminal(&self, out: &mut String) {
        if self.coverage_violations.is_empty() { return; }
        out.push_str(&format!("{} {}\n", "🎯".blue(), format!("Coverage / CRAP Violations ({})", self.coverage_violations.len()).bold().blue()));
        for v in &self.coverage_violations {
            out.push_str(&format!("   • {} - {}: actual {:.1}, limit {:.1}\n     Hint: {}\n", v.file.display().to_string().bold(), v.metric, v.actual, v.limit, v.recommendation.dimmed()));
        }
        out.push('\n');
    }

    fn render_mutation_terminal(&self, out: &mut String) {
        if self.mutation_violations.is_empty() { return; }
        out.push_str(&format!("{} {}\n", "🧬".green(), format!("Mutation Violations ({})", self.mutation_violations.len()).bold().green()));
        for v in &self.mutation_violations {
            out.push_str(&format!("   • {} - {}: actual {:.1}%, floor {:.1}%\n     Hint: {}\n", v.report_file.display().to_string().bold(), v.metric, v.actual, v.limit, v.recommendation.dimmed()));
        }
        out.push('\n');
    }

    fn render_terminal_summary(&self, out: &mut String) {
        let verdict = if self.passed {
            "PASS (All gates satisfied)".bold().green()
        } else {
            format!("FAIL ({} violations detected)", self.total_violations()).bold().red()
        };
        out.push_str(&format!("{}\nSummary: {} files scanned, {} functions analyzed in {}ms.\nVerdict: {}\n", "─".repeat(70).dimmed(), self.files_scanned, self.functions_analyzed, self.duration_ms, verdict));
    }

    pub fn render_agent(&self) -> String {
        if self.passed {
            return format!("✅ **Hardgate Passed**: All {} files and {} functions satisfied strict quality budgets ({}ms).\n", self.files_scanned, self.functions_analyzed, self.duration_ms);
        }

        let mut out = format!("❌ **Hardgate Failed**: {} violations detected across {} files.\n\n", self.total_violations(), self.files_scanned);
        self.render_suppressions_agent(&mut out);
        self.render_complexity_agent(&mut out);
        self.render_budgets_agent(&mut out);
        self.render_invariants_agent(&mut out);
        self.render_clones_agent(&mut out);
        self.render_coverage_agent(&mut out);
        self.render_mutation_agent(&mut out);
        out
    }

    fn render_suppressions_agent(&self, out: &mut String) {
        for v in &self.suppression_violations {
            out.push_str(&format!("### 🚫 Anti-Gaming in `{}:{}`\n- Pragma: `{}`\n- Line: `{}`\n- Directive: Suppressions are prohibited. Fix the underlying compiler/linter error.\n\n", v.file.display(), v.line_number, v.token, v.line_content));
        }
    }

    fn render_complexity_agent(&self, out: &mut String) {
        for v in &self.complexity_violations {
            out.push_str(&format!("### ⚡ Complexity in `{}:{}`\n- Function: `{}`\n- Metric: {} is {:.0} (Budget limit: {:.0})\n- Actionable Refactor: {}\n\n", v.file.display(), v.line_number, v.function_name, v.metric, v.actual, v.limit, v.recommendation));
        }
    }

    fn render_budgets_agent(&self, out: &mut String) {
        for v in &self.budget_violations {
            out.push_str(&format!("### 📦 Physical Budget in `{}`\n- Metric: {}\n- Value: {} (Budget limit: {})\n- Directive: Split this file into cohesive modules.\n\n", v.file.display(), v.metric, v.actual, v.limit));
        }
    }

    fn render_invariants_agent(&self, out: &mut String) {
        for v in &self.invariant_violations {
            out.push_str(&format!("### 🏛️ Architecture in `{}:{}`\n- Rule: `{}`\n- Target: `{}` ({})\n- Requirement: {}\n\n", v.file.display(), v.line_number, v.rule_name, v.offending_target, v.violation_type, v.message));
        }
    }

    fn render_clones_agent(&self, out: &mut String) {
        for v in &self.clone_violations {
            out.push_str(&format!("### 👥 Duplication Clone\n- Loc A: `{}:{}-{}`\n- Loc B: `{}:{}-{}`\n- Span: {} lines, ~{} tokens\n- Refactor: {}\n\n", v.file_a.display(), v.lines_a.0, v.lines_a.1, v.file_b.display(), v.lines_b.0, v.lines_b.1, v.lines, v.tokens, v.recommendation));
        }
    }

    fn render_coverage_agent(&self, out: &mut String) {
        for v in &self.coverage_violations {
            out.push_str(&format!("### 🎯 Coverage / CRAP in `{}`\n- Metric: {} is {:.1} (Threshold: {:.1})\n- Hint: {}\n\n", v.file.display(), v.metric, v.actual, v.limit, v.recommendation));
        }
    }

    fn render_mutation_agent(&self, out: &mut String) {
        for v in &self.mutation_violations {
            out.push_str(&format!("### 🧬 Mutation Floor in `{}`\n- Metric: {} is {:.1}% (Floor: {:.1}%)\n- Hint: {}\n\n", v.report_file.display(), v.metric, v.actual, v.limit, v.recommendation));
        }
    }

    pub fn render_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}
