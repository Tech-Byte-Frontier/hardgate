use super::{GateReport, push_gate_header};
use crate::engines::ComplexityContribution;
use colored::*;

impl GateReport {
    /// Default human output: rustc-style sections per category with details,
    /// contributor breakdowns, and help text.
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
        self.render_orchestration_terminal(&mut out);
        self.render_terminal_summary(&mut out);
        out
    }

    fn render_terminal_header(&self, out: &mut String) {
        push_gate_header(out, &self.gate_name, self.human_status_label());
    }

    fn render_advisories_terminal(&self, out: &mut String) {
        if self.advisories.is_empty() {
            return;
        }
        for advisory in &self.advisories {
            out.push_str(&format!("{} {}\n", "warning:".yellow().bold(), advisory));
        }
        out.push('\n');
    }

    fn render_suppressions_terminal(&self, out: &mut String) {
        if self.suppression_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{}\n",
            format!("error[anti-gaming] ({})", self.suppression_violations.len())
                .bold()
                .red()
        ));
        for v in &self.suppression_violations {
            out.push_str(&format!(
                "  --> {}:{}: forbidden `{}`\n       {}\n",
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
        let groups = self.function_reviews();
        out.push_str(&format!(
            "{}\n",
            format!(
                "error[complexity] ({} functions, {} metric findings)",
                groups.len(),
                self.complexity_violations.len()
            )
            .bold()
            .red()
        ));
        for group in groups {
            out.push_str(&format!(
                "  --> {} [{}]\n",
                group.location().bold(),
                group.function_name.cyan()
            ));
            if let Some(size) = group.size {
                out.push_str(&format!("       size: {}\n", size.description()));
            }
            for v in group.metrics {
                out.push_str(&format!(
                    "       {} is {:.0} (limit: {:.0})\n",
                    v.metric, v.actual, v.limit
                ));
                append_terminal_contributors(&v.breakdown, out);
                out.push_str(&format!(
                    "       {} {}\n",
                    "help:".dimmed(),
                    v.recommendation.dimmed()
                ));
            }
        }
        out.push('\n');
    }

    fn render_budgets_terminal(&self, out: &mut String) {
        if self.budget_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{}\n",
            format!("error[file-budget] ({})", self.budget_violations.len())
                .bold()
                .red()
        ));
        for v in &self.budget_violations {
            out.push_str(&format!(
                "  --> {}: {} is {} (limit: {})\n",
                v.file.display().to_string().bold(),
                v.metric,
                v.actual.to_string().red(),
                v.limit.to_string().green()
            ));
            if let Some(size) = self.file_size_description(&v.file) {
                out.push_str(&format!("       size: {size}\n"));
            }
        }
        out.push('\n');
    }

    fn render_invariants_terminal(&self, out: &mut String) {
        if self.invariant_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{}\n",
            format!("error[architecture] ({})", self.invariant_violations.len())
                .bold()
                .red()
        ));
        for v in &self.invariant_violations {
            out.push_str(&format!(
                "  --> {}:{} [{}]: {}\n       {} target `{}` violates `{}`.\n",
                v.file.display().to_string().bold(),
                v.line_number.to_string().yellow(),
                v.rule_name.bold().red(),
                v.message,
                "help:".dimmed(),
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
            "{}\n",
            format!("error[clone] ({})", self.clone_violations.len())
                .bold()
                .red()
        ));
        for v in &self.clone_violations {
            out.push_str(&format!("  --> `{}:{}-{}` matches `{}:{}-{}` ({} lines, ~{} tokens)\n       {} extract duplicated logic in `{}` and `{}` into a shared helper.\n", v.file_a.display(), v.lines_a.0, v.lines_a.1, v.file_b.display(), v.lines_b.0, v.lines_b.1, v.lines, v.tokens, "help:".dimmed(), v.file_a.display(), v.file_b.display()));
        }
        out.push('\n');
    }

    fn render_coverage_terminal(&self, out: &mut String) {
        if self.coverage_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{}\n",
            format!("error[coverage] ({})", self.coverage_violations.len())
                .bold()
                .red()
        ));
        for v in &self.coverage_violations {
            let func_info = v
                .function_name
                .as_ref()
                .map(|f| format!(" [{}]", f))
                .unwrap_or_default();
            out.push_str(&format!(
                "  --> {}{}: {} is {:.1} (required: {:.1})\n       {} {}\n",
                v.file.display().to_string().bold(),
                func_info.cyan(),
                v.metric,
                v.actual,
                v.limit,
                "help:".dimmed(),
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
            "{}\n",
            format!("error[mutation] ({})", self.mutation_violations.len())
                .bold()
                .red()
        ));
        for v in &self.mutation_violations {
            out.push_str(&format!(
                "  --> {}: {}\n       {} is {:.1} (limit: {:.1})\n       {} {}\n",
                v.report_file.display().to_string().bold(),
                v.message,
                v.metric,
                v.actual,
                v.limit,
                "help:".dimmed(),
                v.recommendation.dimmed()
            ));
        }
        out.push('\n');
    }

    fn render_orchestration_terminal(&self, out: &mut String) {
        self.render_specialist_findings(out);
        if self.orchestration_violations.is_empty() {
            return;
        }
        out.push_str(&format!(
            "{}\n",
            format!("error[tool] ({})", self.orchestration_violations.len())
                .bold()
                .red()
        ));
        for v in &self.orchestration_violations {
            out.push_str(&format!(
                "  --> [{}] `{}` failed (exit: {:?})\n       {}\n       {} {}\n",
                v.step.bold().red(),
                v.command.cyan(),
                v.exit_code,
                v.output.dimmed(),
                "help:".dimmed(),
                v.recommendation.dimmed()
            ));
        }
        out.push('\n');
    }

    pub(crate) fn render_terminal_summary(&self, out: &mut String) {
        let targets = self.function_reviews().len();
        if targets > 0 {
            out.push_str(&format!(
                "review: {targets} functions with {} related metric findings\n",
                self.complexity_violations.len()
            ));
        }
        let code_findings = self.code_findings_count();
        let blockers = self.analysis_blockers_count();
        if self.passed {
            out.push_str(&format!(
                "{}\nsummary: {} files, {} functions in {}ms\nresult: {}\n",
                "-".repeat(70).dimmed(),
                self.files_scanned,
                self.functions_analyzed,
                self.duration_ms,
                self.human_status_label(),
            ));
        } else {
            out.push_str(&format!(
                "{}\nsummary: {} files, {} functions in {}ms ({} code findings, {} analysis blockers)\nresult: {}\n",
                "-".repeat(70).dimmed(),
                self.files_scanned,
                self.functions_analyzed,
                self.duration_ms,
                code_findings,
                blockers,
                self.human_status_label(),
            ));
        }
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
            header: "       key contributors:\n",
            prefix: "         - L",
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
