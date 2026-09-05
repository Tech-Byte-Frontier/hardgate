use super::rules;
use serde::Serialize;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

const MAX_LINES_PER_LOCATION: usize = 8;
const MAX_CHARS_PER_LINE: usize = 240;
const MAX_SNIPPET_BYTES: usize = 64 * 1024;

/// Display-only controls for bounded diagnostics output.
#[derive(Debug, Clone, Default)]
pub struct DisplayOptions {
    pub snippets: bool,
    pub max_diagnostics: Option<usize>,
}

/// One rule diagnostic plus any excerpts captured for its locations.
#[derive(Serialize)]
pub struct DisplayedDiagnostic {
    #[serde(flatten)]
    pub diagnostic: rules::RuleDiagnostic,
    pub excerpts: Vec<SourceExcerpt>,
}

/// Sanitized source lines attached to one diagnostic location.
#[derive(Serialize, Debug, Clone)]
pub struct SourceExcerpt {
    pub file: PathBuf,
    pub first_line: usize,
    pub lines: Vec<String>,
    pub truncated: bool,
}

/// Bounded machine-readable diagnostics prepared for display.
#[derive(Serialize)]
pub struct DiagnosticDisplay {
    pub total: usize,
    pub shown: usize,
    pub omitted: usize,
    pub snippet_bytes: usize,
    pub snippets_truncated: bool,
    pub diagnostics: Vec<DisplayedDiagnostic>,
}

/// Build display diagnostics in the same order as [`rules::diagnostics`].
pub fn diagnostics(report: &super::GateReport) -> DiagnosticDisplay {
    let total = report.total_violations();
    let limit = report.display.max_diagnostics.unwrap_or(usize::MAX);
    let mut budget = SnippetBudget::new(report.display.snippets);
    let diagnostics = rules::diagnostics(report)
        .into_iter()
        .take(limit)
        .map(|diagnostic| displayed_diagnostic(diagnostic, report, &mut budget))
        .collect::<Vec<_>>();
    let shown = diagnostics.len();
    DiagnosticDisplay {
        total,
        shown,
        omitted: total.saturating_sub(shown),
        snippet_bytes: budget.bytes,
        snippets_truncated: budget.truncated,
        diagnostics,
    }
}

/// Limit legacy renderer input without changing the original gate verdict or
/// execution metadata. A report without a display cap is borrowed unchanged.
pub fn report_for_display(report: &super::GateReport) -> Cow<'_, super::GateReport> {
    let Some(limit) = report.display.max_diagnostics else {
        return Cow::Borrowed(report);
    };

    let mut display = report.clone();
    let mut remaining = limit;
    trim_vec(&mut display.budget_violations, &mut remaining);
    trim_vec(&mut display.suppression_violations, &mut remaining);
    trim_vec(&mut display.complexity_violations, &mut remaining);
    trim_vec(&mut display.invariant_violations, &mut remaining);
    trim_vec(&mut display.clone_violations, &mut remaining);
    trim_vec(&mut display.coverage_violations, &mut remaining);
    trim_vec(&mut display.mutation_violations, &mut remaining);
    trim_vec(&mut display.dead_code_violations, &mut remaining);
    trim_vec(&mut display.orchestration_violations, &mut remaining);
    Cow::Owned(display)
}

/// Render bounded diagnostics as plain stable-ID text without a verdict line.
pub fn render_diagnostics(display: &DiagnosticDisplay) -> String {
    let mut output = String::new();
    for item in &display.diagnostics {
        let _ = writeln!(
            output,
            "{} [{}]",
            item.diagnostic.rule_id, item.diagnostic.category
        );
        let _ = writeln!(
            output,
            "message: {}",
            sanitize_controls(&item.diagnostic.message)
        );
        for location in &item.diagnostic.locations {
            let _ = writeln!(output, "location: {}", format_location(location));
        }
        let _ = writeln!(
            output,
            "recommendation: {}",
            sanitize_controls(&item.diagnostic.recommendation)
        );
        render_excerpts(&mut output, &item.excerpts);
        output.push('\n');
    }
    let _ = writeln!(output, "omitted: {} diagnostics", display.omitted);
    output
}

#[derive(Default)]
struct SnippetBudget {
    enabled: bool,
    bytes: usize,
    truncated: bool,
}

impl SnippetBudget {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy)]
struct LineWindow {
    start: usize,
    end: usize,
    first_line: usize,
    truncated: bool,
}

struct LineResult {
    text: String,
    truncated: bool,
    global_truncated: bool,
}

fn displayed_diagnostic(
    diagnostic: rules::RuleDiagnostic,
    report: &super::GateReport,
    budget: &mut SnippetBudget,
) -> DisplayedDiagnostic {
    let excerpts = if budget.enabled {
        diagnostic
            .locations
            .iter()
            .filter_map(|location| excerpt_for_location(location, &report.source_text, budget))
            .collect()
    } else {
        Vec::new()
    };
    DisplayedDiagnostic {
        diagnostic,
        excerpts,
    }
}

fn excerpt_for_location(
    location: &rules::DiagnosticLocation,
    source_text: &BTreeMap<PathBuf, Arc<str>>,
    budget: &mut SnippetBudget,
) -> Option<SourceExcerpt> {
    let source = source_text.get(&location.file)?;
    let window = line_window(location, source.lines().count())?;
    let mut lines = Vec::new();
    let mut truncated = window.truncated;
    if window.truncated {
        budget.truncated = true;
    }

    for raw_line in source
        .lines()
        .skip(window.start)
        .take(window.end - window.start)
    {
        if budget.bytes >= MAX_SNIPPET_BYTES {
            budget.truncated = true;
            truncated = true;
            break;
        }
        let available = MAX_SNIPPET_BYTES.saturating_sub(budget.bytes);
        let line = bounded_line(raw_line, available);
        truncated |= line.truncated;
        budget.truncated |= line.truncated;
        if line.global_truncated {
            budget.truncated = true;
            if line.text.is_empty() && !raw_line.is_empty() {
                break;
            }
        }
        budget.bytes += line.text.len();
        lines.push(line.text);
        if line.global_truncated {
            break;
        }
    }

    (!lines.is_empty()).then_some(SourceExcerpt {
        file: location.file.clone(),
        first_line: window.first_line,
        lines,
        truncated,
    })
}

fn bounded_line(value: &str, max_bytes: usize) -> LineResult {
    let mut output = String::with_capacity(max_bytes.min(MAX_CHARS_PER_LINE * 4));
    let mut output_chars = 0;
    for character in value.chars() {
        let token = visible_token(character);
        let token_chars = token.chars().count();
        if output_chars + token_chars > MAX_CHARS_PER_LINE {
            return LineResult {
                text: output,
                truncated: true,
                global_truncated: false,
            };
        }
        if output.len() + token.len() > max_bytes {
            return LineResult {
                text: output,
                truncated: true,
                global_truncated: true,
            };
        }
        output.push_str(&token);
        output_chars += token_chars;
    }
    LineResult {
        text: output,
        truncated: false,
        global_truncated: false,
    }
}

fn visible_token(character: char) -> String {
    match character {
        '\t' => "\\t".to_string(),
        '\r' => "\\r".to_string(),
        character if character.is_control() => format!("\\u{{{:X}}}", character as u32),
        character => character.to_string(),
    }
}

fn line_window(location: &rules::DiagnosticLocation, line_count: usize) -> Option<LineWindow> {
    if line_count == 0 {
        return None;
    }
    let Some(start_line) = location.line else {
        let end = line_count.min(MAX_LINES_PER_LOCATION);
        return Some(LineWindow {
            start: 0,
            end,
            first_line: 1,
            truncated: line_count > end,
        });
    };
    if start_line == 0 || start_line > line_count {
        return None;
    }
    let requested_end = location.end_line.unwrap_or(start_line);
    if requested_end < start_line {
        return None;
    }
    let capped_end = start_line
        .saturating_add(MAX_LINES_PER_LOCATION.saturating_sub(1))
        .min(line_count)
        .min(requested_end);
    Some(LineWindow {
        start: start_line - 1,
        end: capped_end,
        first_line: start_line,
        truncated: requested_end > capped_end,
    })
}

fn sanitize_controls(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            character if character.is_control() => {
                let _ = write!(output, "\\u{{{:X}}}", character as u32);
            }
            character => output.push(character),
        }
    }
    output
}

fn format_location(location: &rules::DiagnosticLocation) -> String {
    let file = sanitize_controls(&location.file.to_string_lossy());
    match (location.line, location.end_line) {
        (Some(line), Some(end_line)) => format!("{file}:{line}-{end_line}"),
        (Some(line), None) => format!("{file}:{line}"),
        (None, _) => file,
    }
}

fn render_excerpts(output: &mut String, excerpts: &[SourceExcerpt]) {
    for excerpt in excerpts {
        let _ = writeln!(
            output,
            "excerpt: {}:{}",
            sanitize_controls(&excerpt.file.to_string_lossy()),
            excerpt.first_line
        );
        for line in &excerpt.lines {
            let _ = writeln!(output, "  | {line}");
        }
        if excerpt.truncated {
            output.push_str("  | [truncated]\n");
        }
    }
}

fn trim_vec<T>(values: &mut Vec<T>, remaining: &mut usize) {
    let keep = values.len().min(*remaining);
    values.truncate(keep);
    *remaining -= keep;
}

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;

#[cfg(test)]
mod presentation_coverage_tests {
    use super::{
        MAX_CHARS_PER_LINE, MAX_LINES_PER_LOCATION, MAX_SNIPPET_BYTES, SnippetBudget,
        SourceExcerpt, bounded_line, excerpt_for_location, format_location, line_window,
        render_excerpts, rules, sanitize_controls, visible_token,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn location(
        file: &str,
        line: Option<usize>,
        end_line: Option<usize>,
    ) -> rules::DiagnosticLocation {
        rules::DiagnosticLocation {
            file: PathBuf::from(file),
            line,
            end_line,
        }
    }

    #[test]
    fn line_windows_reject_empty_and_invalid_ranges() {
        assert!(line_window(&location("src/a.rs", Some(1), None), 0).is_none());
        assert!(line_window(&location("src/a.rs", Some(0), None), 3).is_none());
        assert!(line_window(&location("src/a.rs", Some(4), None), 3).is_none());
        assert!(line_window(&location("src/a.rs", Some(2), Some(1)), 3).is_none());

        let file_only = line_window(&location("src/a.rs", None, None), 9).unwrap();
        assert_eq!(file_only.first_line, 1);
        assert_eq!(file_only.end, MAX_LINES_PER_LOCATION);
        assert!(file_only.truncated);
    }

    #[test]
    fn control_tokens_locations_and_truncated_excerpts_are_rendered() {
        assert_eq!(visible_token('\0'), "\\u{0}");
        assert_eq!(visible_token('\t'), "\\t");
        assert_eq!(visible_token('\r'), "\\r");
        assert_eq!(sanitize_controls("a\0\t\r"), "a\\u{0}\\t\\r");
        assert_eq!(
            format_location(&location("src/\0.rs", Some(2), Some(4))),
            "src/\\u{0}.rs:2-4"
        );
        assert_eq!(
            format_location(&location("src/a.rs", Some(2), None)),
            "src/a.rs:2"
        );
        assert_eq!(
            format_location(&location("src/a.rs", None, Some(4))),
            "src/a.rs"
        );

        let excerpt = SourceExcerpt {
            file: PathBuf::from("src/\0.rs"),
            first_line: 4,
            lines: vec!["captured".to_string()],
            truncated: true,
        };
        let mut rendered = String::new();
        render_excerpts(&mut rendered, &[excerpt]);
        assert!(rendered.contains("excerpt: src/\\u{0}.rs:4"));
        assert!(rendered.contains("  | captured"));
        assert!(rendered.contains("  | [truncated]"));
    }

    #[test]
    fn bounded_lines_mark_character_and_byte_limits() {
        let characters = bounded_line(&"x".repeat(MAX_CHARS_PER_LINE + 1), usize::MAX);
        assert_eq!(characters.text.chars().count(), MAX_CHARS_PER_LINE);
        assert!(characters.truncated);
        assert!(!characters.global_truncated);

        let bytes = bounded_line("é", 1);
        assert!(bytes.text.is_empty());
        assert!(bytes.truncated);
        assert!(bytes.global_truncated);
    }

    #[test]
    fn global_budget_stops_at_an_unrepresentable_utf8_character() {
        let file = PathBuf::from("src/budget.rs");
        let source = BTreeMap::from([(file.clone(), Arc::<str>::from("é"))]);
        let mut budget = SnippetBudget {
            enabled: true,
            bytes: MAX_SNIPPET_BYTES - 1,
            truncated: false,
        };

        let excerpt = excerpt_for_location(
            &location("src/budget.rs", Some(1), None),
            &source,
            &mut budget,
        );
        assert!(excerpt.is_none());
        assert_eq!(budget.bytes, MAX_SNIPPET_BYTES - 1);
        assert!(budget.truncated);
    }
}
