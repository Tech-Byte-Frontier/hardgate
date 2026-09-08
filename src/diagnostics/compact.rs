use super::{GateReport, push_compact_entry, push_gate_header};
use colored::*;

impl GateReport {
    /// Compact output with one location per function review target and one
    /// entry per other finding, without snippets, breakdowns, or help text.
    pub fn render_compact(&self) -> String {
        let mut out = String::new();
        push_gate_header(&mut out, &self.gate_name, self.human_status_label());
        for advisory in &self.advisories {
            out.push_str(&format!("{} {}\n", "warning:".yellow().bold(), advisory));
        }
        if !self.advisories.is_empty() {
            out.push('\n');
        }
        for (title, target) in self.compact_rows() {
            push_compact_entry(&mut out, title, target);
        }
        self.render_terminal_summary(&mut out);
        out
    }

    fn compact_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        rows.extend(self.compact_primary_rows());
        rows.extend(self.compact_secondary_rows());
        rows.extend(self.compact_tertiary_rows());
        rows
    }

    fn compact_primary_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for v in &self.suppression_violations {
            let title = format!("error[anti-gaming]: forbidden `{}`", v.token);
            rows.push((title, format!("{}:{}", v.file.display(), v.line_number)));
        }
        for group in self.function_reviews() {
            let metrics = group
                .metrics
                .iter()
                .map(|v| format!("{} {:.0}/{:.0}", v.metric, v.actual, v.limit))
                .collect::<Vec<_>>()
                .join("; ");
            let size = group
                .size
                .map(|size| format!("; {}", size.description()))
                .unwrap_or_default();
            rows.push((
                format!(
                    "error[complexity]: `{}`: {metrics}{size}",
                    group.function_name
                ),
                group.location(),
            ));
        }
        for v in &self.budget_violations {
            let title = format!(
                "error[file-budget]: {} is {} (limit: {}){}",
                v.metric,
                v.actual,
                v.limit,
                self.file_size_description(&v.file)
                    .map(|size| format!("; {size}"))
                    .unwrap_or_default()
            );
            rows.push((title, format!("{}", v.file.display())));
        }
        rows
    }

    fn compact_secondary_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for v in &self.invariant_violations {
            let title = format!("error[architecture]: [{}] {}", v.rule_name, v.message);
            let target = format!(
                "{}:{} `{}`",
                v.file.display(),
                v.line_number,
                v.offending_target
            );
            rows.push((title, target));
        }
        for v in &self.clone_violations {
            let title = format!(
                "error[clone]: duplicate ({} lines, ~{} tokens)",
                v.lines, v.tokens
            );
            let target = format!(
                "{}:{}-{} matches {}:{}-{}",
                v.file_a.display(),
                v.lines_a.0,
                v.lines_a.1,
                v.file_b.display(),
                v.lines_b.0,
                v.lines_b.1,
            );
            rows.push((title, target));
        }
        for v in &self.coverage_violations {
            let title = format!(
                "error[coverage]: {} is {:.1} (required: {:.1})",
                v.metric, v.actual, v.limit
            );
            rows.push((title, format!("{}", v.file.display())));
        }
        rows
    }

    fn compact_tertiary_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for v in &self.mutation_violations {
            let title = format!(
                "error[mutation]: {} is {:.1}% (required: {:.1}%)",
                v.metric, v.actual, v.limit
            );
            rows.push((title, format!("{}", v.report_file.display())));
        }

        for finding in &self.tool_diagnostics {
            rows.push((
                format!("{}[{}]: {}", finding.level, finding.rule, finding.message),
                format!(
                    "{}:{}:{}",
                    finding.file.display(),
                    finding.line,
                    finding.column
                ),
            ));
        }
        for v in &self.orchestration_violations {
            let title = format!("error[tool]: `{}` failed", v.command);
            rows.push((title, format!("[{}] `{}`", v.step, v.command)));
        }
        rows
    }
}
