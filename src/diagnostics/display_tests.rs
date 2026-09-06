use super::{DisplayOptions, diagnostics, render_diagnostics, report_for_display};
use crate::diagnostics::GateReport;
use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, CoverageViolation, InvariantViolation,
    SuppressionViolation,
};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

fn report() -> GateReport {
    let mut report = GateReport::new("display-test".to_string());
    report.display = DisplayOptions::default();
    report
}

fn budget(file: &str) -> BudgetViolation {
    BudgetViolation {
        file: PathBuf::from(file),
        metric: "File Byte Size".to_string(),
        actual: 101,
        limit: 100,
        message: "file is too large".to_string(),
    }
}

fn suppression(file: &str, line_number: usize) -> SuppressionViolation {
    SuppressionViolation {
        file: PathBuf::from(file),
        line_number,
        token: "eslint-disable".to_string(),
        line_content: "// eslint-disable".to_string(),
        message: "suppression found".to_string(),
    }
}

fn complexity(file: &str, end_line: usize) -> ComplexityViolation {
    ComplexityViolation {
        file: PathBuf::from(file),
        function_name: "work".to_string(),
        line_number: 1,
        column_number: 0,
        end_line,
        size: None,
        metric: "Function Lines".to_string(),
        actual: 11.0,
        limit: 10.0,
        breakdown: Vec::new(),
        message: "function is too large".to_string(),
        recommendation: "split the function".to_string(),
    }
}

fn invariant(file: &str) -> InvariantViolation {
    InvariantViolation {
        file: PathBuf::from(file),
        line_number: 1,
        rule_name: "boundary".to_string(),
        violation_type: "Disallowed Import".to_string(),
        offending_target: "crate::db".to_string(),
        line_content: "use crate::db;".to_string(),
        message: "architectural boundary crossed".to_string(),
    }
}

fn clone_violation() -> CloneViolation {
    CloneViolation {
        file_a: PathBuf::from("src/a.rs"),
        lines_a: (2, 3),
        file_b: PathBuf::from("src/b.rs"),
        lines_b: (5, 6),
        tokens: 60,
        lines: 2,
        fingerprint: "fingerprint".to_string(),
        message: "duplicate block".to_string(),
        recommendation: "extract the shared helper".to_string(),
    }
}

fn coverage(file: &str) -> CoverageViolation {
    CoverageViolation {
        file: PathBuf::from(file),
        function_name: None,
        metric: "Global Line Coverage".to_string(),
        actual: 40.0,
        limit: 95.0,
        message: "coverage is below the floor".to_string(),
        recommendation: "add tests".to_string(),
    }
}

fn set_source(report: &mut GateReport, file: &str, text: &str) {
    report
        .source_text
        .insert(PathBuf::from(file), Arc::<str>::from(text));
}

#[test]
fn global_limit_spans_categories_and_preserves_verdict_metadata() {
    let mut report = report();
    report.files_scanned = 17;
    report.functions_analyzed = 23;
    report.duration_ms = 41;
    report.passed = false;
    report.budget_violations.push(budget("src/budget.rs"));
    report
        .suppression_violations
        .push(suppression("src/suppression.rs", 2));
    report
        .complexity_violations
        .push(complexity("src/complexity.rs", 3));
    report
        .invariant_violations
        .push(invariant("src/invariant.rs"));
    report.display.max_diagnostics = Some(3);

    let display = diagnostics(&report);
    assert_eq!(display.total, 4);
    assert_eq!(display.shown, 3);
    assert_eq!(display.omitted, 1);
    assert_eq!(display.diagnostics[0].diagnostic.category, "budget");
    assert_eq!(display.diagnostics[2].diagnostic.category, "complexity");
    assert!(render_diagnostics(&display).contains("omitted: 1 diagnostics"));
    assert!(!render_diagnostics(&display).contains("pass"));

    let limited = report_for_display(&report);
    assert!(matches!(&limited, Cow::Owned(_)));
    assert_eq!(limited.total_violations(), 3);
    assert!(!limited.passed);
    assert_eq!(limited.files_scanned, 17);
    assert_eq!(limited.functions_analyzed, 23);
    assert_eq!(limited.duration_ms, 41);
}

#[test]
fn zero_limit_omits_all_findings_without_changing_verdict() {
    let mut report = report();
    report.passed = false;
    report.budget_violations.push(budget("src/budget.rs"));
    report
        .invariant_violations
        .push(invariant("src/invariant.rs"));
    report.display.max_diagnostics = Some(0);

    let display = diagnostics(&report);
    assert_eq!(display.total, 2);
    assert_eq!(display.shown, 0);
    assert_eq!(display.omitted, 2);
    assert!(display.diagnostics.is_empty());

    let limited = report_for_display(&report);
    assert_eq!(limited.total_violations(), 0);
    assert!(!limited.passed);
    assert!(render_diagnostics(&display).contains("omitted: 2 diagnostics"));
}

#[test]
fn no_limit_displays_all_findings_in_stable_order_without_mutating_the_report() {
    let mut report = report();
    report.budget_violations = vec![budget("src/z.rs"), budget("src/a.rs")];
    let displayed = report_for_display(&report);
    assert_eq!(displayed.budget_violations.len(), 2);
    assert_eq!(
        displayed.budget_violations[0].file,
        PathBuf::from("src/a.rs")
    );
    assert_eq!(report.budget_violations[0].file, PathBuf::from("src/z.rs"));
}

#[test]
fn unicode_and_controls_are_visible_and_safe() {
    let mut report = report();
    report.display.snippets = true;
    report
        .suppression_violations
        .push(suppression("src/control.rs", 1));
    set_source(&mut report, "src/control.rs", "pré🙂\u{1b}[31m\t\nsecond");

    let display = diagnostics(&report);
    let line = &display.diagnostics[0].excerpts[0].lines[0];
    assert_eq!(line, "pré🙂\\u{1B}[31m\\t");
    assert!(!line.contains('\u{1b}'));
    let rendered = render_diagnostics(&display);
    assert!(!rendered.contains('\u{1b}'));
    assert!(rendered.contains("\\u{1B}"));
}

#[test]
fn clone_locations_each_receive_their_own_excerpt() {
    let mut report = report();
    report.display.snippets = true;
    report.clone_violations.push(clone_violation());
    set_source(&mut report, "src/a.rs", "zero\nclone-a\nend");
    set_source(
        &mut report,
        "src/b.rs",
        "zero\none\ntwo\nthree\nclone-b\nend",
    );

    let display = diagnostics(&report);
    assert_eq!(display.diagnostics[0].excerpts.len(), 2);
    assert_eq!(
        display.diagnostics[0].excerpts[0].file,
        PathBuf::from("src/a.rs")
    );
    assert_eq!(display.diagnostics[0].excerpts[0].first_line, 2);
    assert_eq!(
        display.diagnostics[0].excerpts[1].file,
        PathBuf::from("src/b.rs")
    );
    assert_eq!(display.diagnostics[0].excerpts[1].first_line, 5);
}

#[test]
fn line_range_and_line_length_limits_are_marked() {
    let mut report = report();
    report.display.snippets = true;
    report
        .suppression_violations
        .push(suppression("src/long.rs", 1));
    report
        .complexity_violations
        .push(complexity("src/range.rs", 12));
    let long_line = "é".repeat(1_000_000);
    set_source(&mut report, "src/long.rs", &long_line);
    set_source(
        &mut report,
        "src/range.rs",
        &(1..=12)
            .map(|line| format!("line-{line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let display = diagnostics(&report);
    let long = &display.diagnostics[0].excerpts[0];
    assert_eq!(long.lines[0].chars().count(), 240);
    assert!(long.truncated);
    let range = &display.diagnostics[1].excerpts[0];
    assert_eq!(range.lines.len(), 8);
    assert!(range.truncated);
    assert!(display.snippets_truncated);
}

#[test]
fn global_snippet_budget_is_utf8_safe() {
    let mut report = report();
    report.display.snippets = true;
    for index in 0..20 {
        let file = format!("src/large-{index}.rs");
        report.coverage_violations.push(coverage(&file));
        let source = vec!["é".repeat(240); 8].join("\n");
        set_source(&mut report, &file, &source);
    }

    let display = diagnostics(&report);
    assert_eq!(display.snippet_bytes, 64 * 1024);
    assert!(display.snippets_truncated);
    assert!(display.diagnostics.iter().any(|diagnostic| {
        diagnostic.excerpts.iter().any(|excerpt| {
            excerpt.truncated && excerpt.lines.iter().any(|line| line.chars().count() < 240)
        })
    }));
    assert!(
        display
            .diagnostics
            .iter()
            .flat_map(|diagnostic| &diagnostic.excerpts)
            .flat_map(|excerpt| &excerpt.lines)
            .all(|line| line.chars().all(|character| character == 'é'))
    );
}

#[test]
fn absent_map_omits_excerpt_but_captured_bytes_ignore_filesystem_state() {
    let mut absent = report();
    absent.display.snippets = true;
    absent.budget_violations.push(budget("missing/on/disk.rs"));
    let without_source = diagnostics(&absent);
    assert!(without_source.diagnostics[0].excerpts.is_empty());
    assert_eq!(without_source.snippet_bytes, 0);

    let mut captured = report();
    captured.display.snippets = true;
    captured
        .budget_violations
        .push(budget("missing/on/disk.rs"));
    set_source(&mut captured, "missing/on/disk.rs", "captured bytes");
    let with_source = diagnostics(&captured);
    assert_eq!(
        with_source.diagnostics[0].excerpts[0].lines[0],
        "captured bytes"
    );
}
