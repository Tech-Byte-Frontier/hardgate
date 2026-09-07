use super::agent::short_text;
use super::{GateReport, display};
use crate::commands::CommandOutcome;
use std::fmt::Write;

impl GateReport {
    pub(crate) fn render_acceptance_context(&self) -> String {
        let outcome = CommandOutcome::from_report(self);
        let title = if self.passed {
            "✅ **Hardgate Passed**"
        } else {
            "❌ **Hardgate Failed**"
        };
        let mut out = format!(
            "{title} ({}; {}; exit {})\n",
            if self.passed { "pass" } else { "fail" },
            outcome.status(),
            outcome.exit_code()
        );
        render_scope(self, &mut out);
        render_counts(self, &mut out);
        // Failure context is outside the display filter/cap, including a zero cap.
        for failure in self.failure_diagnostics() {
            let locations = failure
                .locations
                .iter()
                .map(display::format_location)
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                out,
                "error[tool]: {} [{}] {locations}: {}",
                failure
                    .rule_id
                    .strip_prefix("HG-ORCHESTRATION-")
                    .unwrap_or(&failure.category)
                    .to_ascii_lowercase(),
                failure.rule_id,
                short_text(&failure.message)
            );
            let _ = writeln!(
                out,
                "  {}",
                display::sanitize_controls(&failure.recommendation)
            );
        }
        out
    }
}

fn render_scope(report: &GateReport, out: &mut String) {
    if let Some(plan) = &report.execution {
        let scope = if plan.scope.paths.is_empty() {
            plan.config.root.display().to_string()
        } else {
            plan.scope
                .paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(
            out,
            "Scope: {} [{}]; {} files and {} functions; {}.",
            plan.scope.mode,
            scope,
            report.files_scanned,
            report.functions_analyzed,
            if plan.is_partial() {
                "partial: not complete acceptance"
            } else if report.passed && plan.is_complete() {
                "complete acceptance"
            } else {
                "acceptance failed"
            }
        );
        let selected = plan
            .engines
            .iter()
            .filter(|engine| engine.selected)
            .map(|engine| format!("{:?}={:?}", engine.id, engine.state))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "Evaluated engines: {selected}");
        let _ = writeln!(
            out,
            "Omitted requirements: {:?}",
            plan.omitted_requirements()
        );
    } else {
        let _ = writeln!(
            out,
            "Scope: {} files and {} functions; execution metadata unavailable, complete acceptance unproven.",
            report.files_scanned, report.functions_analyzed
        );
    }
}

fn render_counts(report: &GateReport, out: &mut String) {
    let s = report.summary();
    let _ = writeln!(
        out,
        "Totals: {} errors (budget {}, suppression {}, complexity {}, invariant {}, clones {}, coverage {}, mutation {}, specialist {}, tool {}); {} advisories.",
        s.total_errors,
        s.file_budgets,
        s.suppressions,
        s.complexity,
        s.architecture,
        s.clones,
        s.coverage,
        s.mutation,
        s.specialist_findings,
        s.tool,
        report.advisories.len()
    );
}
