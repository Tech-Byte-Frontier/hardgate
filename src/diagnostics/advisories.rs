use super::{GateReport, display};
use std::collections::BTreeMap;
use std::fmt::Write;

impl GateReport {
    pub(super) fn render_grouped_advisories(&self, out: &mut String) {
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for advisory in &self.advisories {
            let (key, location) = inventory_advisory(advisory)
                .or_else(|| clone_advisory(advisory))
                .unwrap_or((advisory.clone(), String::new()));
            groups.entry(key).or_default().push(location);
        }
        for finding in self
            .tool_diagnostics
            .iter()
            .filter(|finding| !finding.blocking)
        {
            groups
                .entry(format!("{}: {}", finding.rule, finding.message))
                .or_default()
                .push(format!(
                    "{}:{}:{}",
                    finding.file.display(),
                    finding.line,
                    finding.column
                ));
        }
        let total = groups.len();
        let limit = self.display.max_diagnostics.unwrap_or(usize::MAX);
        for (message, locations) in groups.into_iter().take(limit) {
            let examples = locations
                .iter()
                .filter(|location| !location.is_empty())
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let suffix = if locations.len() > 1 {
                format!(" ({} occurrences)", locations.len())
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "Advisory: {}{suffix}{}",
                display::sanitize_controls(&message),
                if examples.is_empty() {
                    String::new()
                } else {
                    format!(
                        " [{}{}]",
                        display::sanitize_controls(&examples),
                        if locations.len() > 3 {
                            ", …; full locations in JSON"
                        } else {
                            ""
                        }
                    )
                }
            );
        }
        if total > limit {
            let _ = writeln!(
                out,
                "Advisories: {} groups omitted by --max-diagnostics; complete advisories remain in JSON.",
                total - limit
            );
        }
    }
}

fn clone_advisory(advisory: &str) -> Option<(String, String)> {
    let (role, rest) = advisory.split_once(" advisory: clone for `")?;
    let (path, detail) = rest.split_once("`: ")?;
    Some((
        format!("{role} clone advisories under the configured clone policy"),
        format!("{path}: {detail}"),
    ))
}

fn inventory_advisory(advisory: &str) -> Option<(String, String)> {
    let rest = advisory.strip_prefix("role Source: `")?;
    let (path, message) =
        rest.split_once("` is a recognized inventory source without AST parser;")?;
    Some((
        format!("Recognized inventory source without AST parser;{}", message),
        path.into(),
    ))
}
