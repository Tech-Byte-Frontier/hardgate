use super::GateReport;
use crate::engines::ComplexityContribution;

impl GateReport {
    /// Structured Markdown for LLM context windows: pinpoint breakdowns with
    /// actionable refactor directives instead of terminal styling.
    pub fn render_agent(&self) -> String {
        if self.passed {
            let mut out = format!(
                "✅ **Hardgate Passed**: All {} files and {} functions satisfied strict quality budgets ({}ms).\n\n",
                self.files_scanned, self.functions_analyzed, self.duration_ms
            );
            self.render_advisories_agent(&mut out);
            return out;
        }

        let mut out = format!(
            "❌ **Hardgate Failed**: {} violations detected across {} files.\n\n",
            self.total_violations(),
            self.files_scanned
        );
        self.render_advisories_agent(&mut out);
        self.render_suppressions_agent(&mut out);
        self.render_complexity_agent(&mut out);
        self.render_budgets_agent(&mut out);
        self.render_invariants_agent(&mut out);
        self.render_clones_agent(&mut out);
        self.render_coverage_agent(&mut out);
        self.render_mutation_agent(&mut out);
        self.render_dead_code_agent(&mut out);
        self.render_orchestration_agent(&mut out);
        out
    }

    fn render_advisories_agent(&self, out: &mut String) {
        if self.advisories.is_empty() {
            return;
        }
        for advisory in &self.advisories {
            out.push_str(&format!("> ⚠️ **Advisory**: {}\n\n", advisory));
        }
    }

    fn render_suppressions_agent(&self, out: &mut String) {
        for v in &self.suppression_violations {
            out.push_str(&format!(
                "### 🚫 Anti-Gaming in `{}:{}`\n- Pragma: `{}`\n- Line: `{}`\n- Directive: Suppressions are prohibited. Fix the underlying compiler/linter error.\n\n",
                v.file.display(), v.line_number, v.token, v.line_content
            ));
        }
    }

    fn render_complexity_agent(&self, out: &mut String) {
        for v in &self.complexity_violations {
            out.push_str(&format!(
                "### ⚡ Complexity in `{}:{}`\n- Function: `{}`\n- Metric: {} is {:.0} (Budget limit: {:.0})\n",
                v.file.display(), v.line_number, v.function_name, v.metric, v.actual, v.limit
            ));
            append_agent_contributors(&v.breakdown, out);
            out.push_str(&format!("- Actionable Refactor: {}\n\n", v.recommendation));
        }
    }

    fn render_budgets_agent(&self, out: &mut String) {
        for v in &self.budget_violations {
            out.push_str(&format!(
                "### 📦 Physical Budget in `{}`\n- Metric: {}\n- Value: {} (Budget limit: {})\n- Directive: Split this file into cohesive modules.\n\n",
                v.file.display(), v.metric, v.actual, v.limit
            ));
        }
    }

    fn render_invariants_agent(&self, out: &mut String) {
        for v in &self.invariant_violations {
            out.push_str(&format!(
                "### 🏛️ Architecture in `{}:{}`\n- Rule: `{}`\n- Target: `{}` ({})\n- Requirement: {}\n\n",
                v.file.display(), v.line_number, v.rule_name, v.offending_target, v.violation_type, v.message
            ));
        }
    }

    fn render_clones_agent(&self, out: &mut String) {
        for v in &self.clone_violations {
            out.push_str(&format!(
                "### 👥 Duplication Clone\n- Loc A: `{}:{}-{}`\n- Loc B: `{}:{}-{}`\n- Span: {} lines, ~{} tokens\n- Refactor: {}\n\n",
                v.file_a.display(), v.lines_a.0, v.lines_a.1, v.file_b.display(), v.lines_b.0, v.lines_b.1, v.lines, v.tokens, v.recommendation
            ));
        }
    }

    fn render_coverage_agent(&self, out: &mut String) {
        for v in &self.coverage_violations {
            out.push_str(&format!(
                "### 🎯 Coverage / CRAP in `{}`\n- Metric: {} is {:.1} (Threshold: {:.1})\n- Hint: {}\n\n",
                v.file.display(), v.metric, v.actual, v.limit, v.recommendation
            ));
        }
    }

    fn render_mutation_agent(&self, out: &mut String) {
        for v in &self.mutation_violations {
            out.push_str(&format!(
                "### 🧬 Mutation Floor in `{}`\n- Metric: {} is {:.1}% (Floor: {:.1}%)\n- Hint: {}\n\n",
                v.report_file.display(), v.metric, v.actual, v.limit, v.recommendation
            ));
        }
    }

    fn render_dead_code_agent(&self, out: &mut String) {
        for v in &self.dead_code_violations {
            out.push_str(&format!(
                "### 🍂 Dead Code in `{}`\n- Type: `{}`\n- Details: {}\n- Directive: {}\n\n",
                v.file.display(),
                v.violation_type,
                v.message,
                v.recommendation
            ));
        }
    }

    fn render_orchestration_agent(&self, out: &mut String) {
        for v in &self.orchestration_violations {
            out.push_str(&format!(
                "### 🛠️ Tool Failure: `{}`\n- Command: `{}`\n- Exit Code: {:?}\n- Output:\n```text\n{}\n```\n- Directive: {}\n\n",
                v.step, v.command, v.exit_code, v.output, v.recommendation
            ));
        }
    }
}

fn append_agent_contributors(breakdown: &[ComplexityContribution], out: &mut String) {
    if breakdown.is_empty() {
        return;
    }
    out.push_str("- Key AST Contributors:\n");
    for b in breakdown {
        out.push_str(&format!(
            "  - Line {}: +{} for {}\n",
            b.line, b.score, b.description
        ));
    }
}
