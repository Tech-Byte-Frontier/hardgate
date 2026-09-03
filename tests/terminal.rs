#[path = "support/reports.rs"]
mod reports;

use hardgate::GateReport;
use hardgate::commands::{MutationSummaryContext, format_mutation_terminal};
use hardgate::engines::{AstMutant, MutantExecutionResult, MutantOutcome, MutationStats};
use reports::failing_report;
use std::path::PathBuf;

fn tally(killed: usize, survived: usize) -> MutationStats {
    MutationStats {
        killed,
        survived,
        timeout: 0,
        unviable: 0,
        total: killed + survived,
    }
}

fn execution(line: usize, outcome: MutantOutcome) -> MutantExecutionResult {
    MutantExecutionResult {
        mutant: AstMutant {
            id: line,
            file: PathBuf::from("src/main.rs"),
            line,
            column: 1,
            start_byte: 0,
            end_byte: 2,
            original: "==".to_string(),
            replacement: "!=".to_string(),
            description: "Replace == with !=".to_string(),
        },
        outcome,
        duration_ms: 5,
        command: "cargo test".to_string(),
    }
}

fn summarize<'a>(
    stats: &'a MutationStats,
    results: &'a [MutantExecutionResult],
    score: f64,
    passed: bool,
) -> MutationSummaryContext<'a> {
    MutationSummaryContext {
        stats,
        results,
        score,
        min_score: 85.0,
        passed,
        elapsed: 9,
    }
}

#[test]
fn test_terminal_pass_report() {
    let mut report = GateReport::new("demo".to_string());
    report.finalize(10, 50, 42);

    assert!(report.passed);
    let term = report.render_terminal();
    for needle in [
        "hardgate [demo]",
        "pass",
        "summary: 10 files, 50 functions in 42ms",
        "result: pass",
    ] {
        assert!(term.contains(needle), "missing {needle}");
    }
    assert!(!term.contains("error["));
    assert!(!term.contains("fail"));
}

#[test]
fn test_terminal_fail_report() {
    let mut report = failing_report();
    report.finalize(3, 12, 7);

    assert!(!report.passed);
    let term = report.render_terminal();
    for needle in [
        "error[complexity]",
        "error[file-budget]",
        "-->",
        "help:",
        "src/main.rs",
        "src/big.rs",
        "result: fail (2 errors)",
    ] {
        assert!(term.contains(needle), "missing {needle}");
    }
    assert!(!term.contains("result: pass"));
}

#[test]
fn test_mutation_terminal_pass_repeats_verdict_at_end() {
    let stats = tally(2, 0);
    let results = vec![
        execution(1, MutantOutcome::Killed),
        execution(2, MutantOutcome::Killed),
    ];
    let out = format_mutation_terminal(&summarize(&stats, &results, 100.0, true));

    assert!(out.contains("mutation summary:"));
    assert_eq!(out.matches("result:").count(), 2);
    assert_closing_contains(&out, &["pass", "100.0%"]);
}

#[test]
fn test_mutation_terminal_fail_lists_survivors_then_verdict() {
    let stats = tally(1, 1);
    let results = vec![
        execution(1, MutantOutcome::Killed),
        execution(7, MutantOutcome::Survived),
    ];
    let out = format_mutation_terminal(&summarize(&stats, &results, 50.0, false));

    assert!(out.contains("survived mutants (1)"));
    assert!(out.contains("src/main.rs:7"));
    assert_eq!(out.matches("result:").count(), 2);
    assert_closing_contains(
        &out,
        &["fail", "50.0%", "1 killed, 1 survived, 0 timed out"],
    );
}

fn assert_closing_contains(out: &str, needles: &[&str]) {
    let closing = out.lines().last().unwrap();
    for needle in needles {
        assert!(
            closing.contains(needle),
            "closing `{closing}` lacks {needle}"
        );
    }
}
