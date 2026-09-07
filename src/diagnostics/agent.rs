use super::{GateReport, display, rules};
use std::fmt::Write;

impl GateReport {
    /// Concise review targets with complete acceptance context.
    pub fn render_agent(&self) -> String {
        self.render_triage(true)
    }

    pub(crate) fn render_triage(&self, guidance: bool) -> String {
        self.render_triage_with_context(self, guidance)
    }

    pub(crate) fn render_triage_with_context(&self, original: &Self, guidance: bool) -> String {
        let mut out = original.render_acceptance_context();
        let shown = display::diagnostics(self);
        let visible = display::report_for_display(self);
        render_items(&mut out, &shown, &visible, guidance);
        let _ = writeln!(
            out,
            "Displayed: {}/{} findings; omitted: {} diagnostics.",
            shown.shown,
            original.summary().total_errors,
            original.summary().total_errors.saturating_sub(shown.shown)
        );
        if original.summary().total_errors > shown.shown {
            out.push_str("Inspect omitted findings: remove --engine/--metric/--top/--max-diagnostics, or run hardgate report <complete.json> --format agent. Save all findings with --report-json <complete.json>.\n");
        }
        if self.display.snippets
            && shown
                .diagnostics
                .iter()
                .any(|item| !item.diagnostic.locations.is_empty() && item.excerpts.is_empty())
        {
            out.push_str("Excerpts unavailable for some locations; only captured report excerpts are used during saved-report inspection.\n");
        }
        original.render_grouped_advisories(&mut out);
        if self.display.details {
            out.push_str(&visible.render_agent_details());
        }
        out
    }
}

fn render_finding(
    out: &mut String,
    item: &display::DisplayedDiagnostic,
    report: &GateReport,
    guidance: bool,
) {
    let diagnostic = &item.diagnostic;
    let locations = diagnostic
        .locations
        .iter()
        .map(display::format_location)
        .collect::<Vec<_>>()
        .join(" <-> ");
    let _ = writeln!(out, "{locations} {}", diagnostic.rule_id);
    let message =
        measurement(report, diagnostic).unwrap_or_else(|| short_text(&diagnostic.message));
    let _ = writeln!(out, "  {}", display::sanitize_controls(&message));
    if guidance {
        let review = match diagnostic.category.as_str() {
            "clone" => "Review whether the duplicated logic has one shared responsibility.",
            "budget" => {
                "Review code and documentation separately before extracting cohesive modules."
            }
            _ => &diagnostic.recommendation,
        };
        let _ = writeln!(out, "  Review: {}", display::sanitize_controls(review));
    }
    display::render_excerpts(out, &item.excerpts);
}

pub(super) fn short_text(text: &str) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let mut shortened: String = line.chars().take(240).collect();
    if shortened.len() < text.len() {
        shortened.push_str(" … (--details)");
    }
    display::sanitize_controls(&shortened)
}

fn render_items(
    out: &mut String,
    shown: &display::DiagnosticDisplay,
    visible: &GateReport,
    guidance: bool,
) {
    let mut functions = std::collections::BTreeSet::new();
    for item in &shown.diagnostics {
        if item.diagnostic.category == "complexity" {
            let location = &item.diagnostic.locations[0];
            if functions.insert((location.file.clone(), location.line, location.end_line)) {
                render_function(out, visible, location, guidance);
                display::render_excerpts(out, &item.excerpts);
            }
        } else {
            render_finding(out, item, visible, guidance);
        }
    }
}

fn measurement(report: &GateReport, diagnostic: &rules::RuleDiagnostic) -> Option<String> {
    match diagnostic.category.as_str() {
        "clone" => clone_measurement(report, diagnostic),
        "budget" => budget_measurement(report, diagnostic),
        "coverage" => report
            .coverage_violations
            .iter()
            .find(|finding| {
                finding.message == diagnostic.message
                    && diagnostic.locations[0].file == finding.file
            })
            .map(|finding| {
                format!(
                    "{} {:.1}/{:.1}",
                    finding.metric, finding.actual, finding.limit
                )
            }),
        "mutation" => Some(diagnostic.message.clone()),
        _ => None,
    }
}

fn clone_measurement(report: &GateReport, diagnostic: &rules::RuleDiagnostic) -> Option<String> {
    report
        .clone_violations
        .iter()
        .find(|finding| {
            diagnostic.locations[0].file == finding.file_a
                && diagnostic.locations[0].line == Some(finding.lines_a.0)
                && diagnostic.locations[0].end_line == Some(finding.lines_a.1)
                && diagnostic.locations[1].file == finding.file_b
                && diagnostic.locations[1].line == Some(finding.lines_b.0)
                && diagnostic.locations[1].end_line == Some(finding.lines_b.1)
        })
        .map(|finding| {
            format!(
                "duplicate: {} lines, ~{} tokens",
                finding.lines, finding.tokens
            )
        })
}

fn budget_measurement(report: &GateReport, diagnostic: &rules::RuleDiagnostic) -> Option<String> {
    report
        .budget_violations
        .iter()
        .find(|finding| {
            finding.message == diagnostic.message && diagnostic.locations[0].file == finding.file
        })
        .map(|finding| {
            format!(
                "{} {}/{}{}",
                finding.metric,
                finding.actual,
                finding.limit,
                report
                    .file_size_description(&finding.file)
                    .map(|size| format!("; {size}"))
                    .unwrap_or_default()
            )
        })
}

fn render_function(
    out: &mut String,
    report: &GateReport,
    location: &rules::DiagnosticLocation,
    guidance: bool,
) {
    for group in report.function_reviews().into_iter().filter(|group| {
        group.file == location.file
            && Some(group.line) == location.line
            && Some(group.end_line) == location.end_line
    }) {
        let _ = writeln!(out, "{} `{}`", group.location(), group.function_name);
        if let Some(size) = group.size {
            let _ = writeln!(out, "  {}", size.description());
        }
        for metric in group.metrics {
            let _ = writeln!(
                out,
                "  {}: {} {:.0}/{:.0}",
                rules::complexity_rule_id(&metric.metric),
                metric.metric,
                metric.actual,
                metric.limit
            );
        }
        if guidance {
            out.push_str("  Review: simplify the shared control flow; preserve behavior.\n");
        }
    }
}
