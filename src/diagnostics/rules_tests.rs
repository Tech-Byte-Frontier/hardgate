use super::diagnostics;
use crate::diagnostics::GateReport;
use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, CoverageViolation, DeadCodeViolation,
    InvariantViolation, MutationViolation, OrchestrationViolation, SuppressionViolation,
};
use std::path::PathBuf;

fn report() -> GateReport {
    GateReport::new("rules-test".to_string())
}

fn budget(metric: &str) -> BudgetViolation {
    BudgetViolation {
        file: PathBuf::from("src/budget.rs"),
        metric: metric.to_string(),
        actual: 101,
        limit: 100,
        message: format!("{metric} exceeded"),
    }
}

fn suppression() -> SuppressionViolation {
    SuppressionViolation {
        file: PathBuf::from("src/suppression.rs"),
        line_number: 4,
        token: "eslint-disable".to_string(),
        line_content: "// eslint-disable".to_string(),
        message: "suppression found".to_string(),
    }
}

fn complexity(metric: &str) -> ComplexityViolation {
    ComplexityViolation {
        file: PathBuf::from("src/complexity.rs"),
        function_name: "work".to_string(),
        line_number: 5,
        end_line: 12,
        metric: metric.to_string(),
        actual: 11.0,
        limit: 10.0,
        breakdown: Vec::new(),
        message: format!("{metric} exceeded"),
        recommendation: "split the function".to_string(),
    }
}

fn invariant(kind: &str) -> InvariantViolation {
    InvariantViolation {
        file: PathBuf::from("src/invariant.rs"),
        line_number: 6,
        rule_name: "no-db".to_string(),
        violation_type: kind.to_string(),
        offending_target: "crate::db".to_string(),
        line_content: "use crate::db;".to_string(),
        message: "architectural boundary crossed".to_string(),
    }
}

fn clone_violation(
    a: &str,
    a_lines: (usize, usize),
    b: &str,
    b_lines: (usize, usize),
) -> CloneViolation {
    CloneViolation {
        file_a: PathBuf::from(a),
        lines_a: a_lines,
        file_b: PathBuf::from(b),
        lines_b: b_lines,
        tokens: 60,
        lines: 6,
        fingerprint: "stable-fingerprint".to_string(),
        message: "duplicate block".to_string(),
        recommendation: "extract the shared helper".to_string(),
    }
}

fn coverage(metric: &str) -> CoverageViolation {
    CoverageViolation {
        file: PathBuf::from("src/coverage.rs"),
        function_name: None,
        metric: metric.to_string(),
        actual: 40.0,
        limit: 95.0,
        message: format!("{metric} below floor"),
        recommendation: "add tests".to_string(),
    }
}

fn mutation(metric: &str) -> MutationViolation {
    MutationViolation {
        report_file: PathBuf::from("reports/mutation.json"),
        metric: metric.to_string(),
        actual: 40.0,
        limit: 85.0,
        message: format!("{metric} failed"),
        recommendation: "repair mutation evidence".to_string(),
    }
}

fn dead_code(kind: &str, line_number: Option<usize>) -> DeadCodeViolation {
    DeadCodeViolation {
        file: PathBuf::from("src/dead.rs"),
        line_number,
        symbol: Some("unused".to_string()),
        violation_type: kind.to_string(),
        message: "dead code".to_string(),
        recommendation: "remove the unused code".to_string(),
    }
}

fn orchestration(step: &str) -> OrchestrationViolation {
    OrchestrationViolation {
        step: step.to_string(),
        command: "tool check".to_string(),
        exit_code: Some(1),
        output: "tool failed".to_string(),
        recommendation: "repair the tool failure".to_string(),
    }
}

fn add_orchestration_variants(report: &mut GateReport) {
    for step in [
        "format_check",
        "format",
        "lint",
        "test",
        "generated-freshness",
        "coverage-diff",
        "coverage-report",
        "coverage-source-classification",
        "dead-code-context",
        "read-clone-index",
        "clone-index",
        "read-source",
        "parse-source",
        "classify-source",
        "unsupported-source",
        "mutation-report",
        "legacy-ratchet",
    ] {
        report.orchestration_violations.push(orchestration(step));
    }
}

#[test]
fn representative_report_preserves_category_order_and_locations() {
    let mut report = report();
    report.budget_violations.push(budget("File Byte Size"));
    report.suppression_violations.push(suppression());
    report
        .complexity_violations
        .push(complexity("Cyclomatic Complexity"));
    report
        .invariant_violations
        .push(invariant("Disallowed Import"));
    report
        .clone_violations
        .push(clone_violation("src/a.rs", (7, 12), "src/b.rs", (20, 25)));
    report
        .coverage_violations
        .push(coverage("Global Line Coverage"));
    report
        .mutation_violations
        .push(mutation("Mutation Kill Rate"));
    report
        .dead_code_violations
        .push(dead_code("Unused Export", Some(8)));
    report.orchestration_violations.push(orchestration("lint"));

    let found = diagnostics(&report);
    assert_eq!(found.len(), 9);
    assert_eq!(
        found
            .iter()
            .map(|item| item.category.as_str())
            .collect::<Vec<_>>(),
        vec![
            "budget",
            "suppression",
            "complexity",
            "invariant",
            "clone",
            "coverage",
            "mutation",
            "dead-code",
            "orchestration",
        ]
    );
    assert_eq!(found[0].rule_id, "HG-BUDGET-FILE-BYTE-SIZE");
    assert_eq!(found[1].locations[0].line, Some(4));
    assert_eq!(found[2].locations[0].end_line, Some(12));
    assert_eq!(found[4].rule_id, "HG-CLONE-DUPLICATE-BLOCK");
    assert_eq!(found[4].locations.len(), 2);
    assert_eq!(found[4].locations[1].line, Some(20));
    assert_eq!(found[5].locations[0].line, None);
    assert_eq!(found[6].locations[0].line, None);
    assert_eq!(found[7].locations[0].line, Some(8));
    assert!(found[8].locations.is_empty());
    assert_eq!(found[2].recommendation, "split the function");
    assert_eq!(found[7].recommendation, "remove the unused code");
}

#[test]
fn live_metric_variants_have_explicit_ids() {
    let mut report = report();
    for metric in ["File Byte Size", "Physical Lines (.rs)"] {
        report.budget_violations.push(budget(metric));
    }
    for metric in [
        "Cyclomatic Complexity",
        "Cognitive Complexity",
        "Parameter Count",
        "Function Lines",
        "Nesting Depth",
        "Halstead Difficulty",
        "Statement Count",
        "ABC Score",
    ] {
        report.complexity_violations.push(complexity(metric));
    }
    for kind in ["Disallowed Import", "Disallowed Call", "Disallowed Token"] {
        report.invariant_violations.push(invariant(kind));
    }
    for metric in [
        "Coverage Count Overflow",
        "Global Line Coverage",
        "Global Function Coverage",
        "Global Branch Coverage",
        "Missing Source Coverage",
        "CRAP Score",
        "Missing Critical Path",
        "Critical Path 100% Coverage",
        "Missing Diff Coverage",
        "Diff Line Coverage",
    ] {
        report.coverage_violations.push(coverage(metric));
    }
    for metric in [
        "Mutation Kill Rate",
        "Mutation Timeouts",
        "Mutation Compile Errors",
        "Mutation Runner Errors",
        "Mutation Unviable Mutants",
    ] {
        report.mutation_violations.push(mutation(metric));
    }
    for kind in ["Unreferenced File", "Unused Export"] {
        report.dead_code_violations.push(dead_code(kind, Some(1)));
    }
    add_orchestration_variants(&mut report);

    let found = diagnostics(&report);
    assert!(found.iter().all(|item| item.rule_id.starts_with("HG-")));
    assert_eq!(found[0].rule_id, "HG-BUDGET-FILE-BYTE-SIZE");
    assert_eq!(found[1].rule_id, "HG-BUDGET-PHYSICAL-LINES");
    assert_eq!(found[2].rule_id, "HG-COMPLEXITY-CYCLOMATIC");
    assert_eq!(found[10].rule_id, "HG-INVARIANT-DISALLOWED-IMPORT");
    assert_eq!(found[13].rule_id, "HG-COVERAGE-COUNT-OVERFLOW");
    assert_eq!(found[23].rule_id, "HG-MUTATION-KILL-RATE");
    assert_eq!(found[28].rule_id, "HG-DEAD-CODE-UNREFERENCED-FILE");
    assert_eq!(found[30].rule_id, "HG-ORCHESTRATION-FORMAT-CHECK");
    assert_eq!(
        found[37].rule_id,
        "HG-ORCHESTRATION-COVERAGE-SOURCE-CLASSIFICATION"
    );
    assert_eq!(found[46].rule_id, "HG-ORCHESTRATION-LEGACY-RATCHET");
}

#[test]
fn unknown_values_use_category_fallback_ids() {
    let mut report = report();
    report.budget_violations.push(budget("Future File Metric"));
    report
        .complexity_violations
        .push(complexity("Future Complexity"));
    report
        .invariant_violations
        .push(invariant("Future Invariant"));
    report.coverage_violations.push(coverage("Future Coverage"));
    report.mutation_violations.push(mutation("Future Mutation"));
    report
        .dead_code_violations
        .push(dead_code("Future Dead Code", None));
    report
        .orchestration_violations
        .push(orchestration("future-step"));

    let found = diagnostics(&report);
    assert_eq!(
        found
            .iter()
            .map(|item| item.rule_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "HG-BUDGET-UNKNOWN-METRIC",
            "HG-COMPLEXITY-UNKNOWN-METRIC",
            "HG-INVARIANT-UNKNOWN-KIND",
            "HG-COVERAGE-UNKNOWN-METRIC",
            "HG-MUTATION-UNKNOWN-METRIC",
            "HG-DEAD-CODE-UNKNOWN-KIND",
            "HG-ORCHESTRATION-UNKNOWN-STEP",
        ]
    );
}

#[test]
fn clone_rule_id_is_independent_of_path_and_line_changes() {
    let mut first = report();
    first.clone_violations.push(clone_violation(
        "src/old_a.rs",
        (3, 8),
        "src/old_b.rs",
        (20, 25),
    ));
    let mut second = report();
    second.clone_violations.push(clone_violation(
        "src/renamed_a.rs",
        (40, 45),
        "src/renamed_b.rs",
        (90, 95),
    ));

    let first_diagnostics = diagnostics(&first);
    let second_diagnostics = diagnostics(&second);
    let first_diagnostic = &first_diagnostics[0];
    let second_diagnostic = &second_diagnostics[0];
    assert_eq!(first_diagnostic.rule_id, second_diagnostic.rule_id);
    assert_eq!(
        first_diagnostic.locations[0].file,
        PathBuf::from("src/old_a.rs")
    );
    assert_eq!(first_diagnostic.locations[0].line, Some(3));
    assert_eq!(first_diagnostic.locations[0].end_line, Some(8));
    assert_eq!(
        second_diagnostic.locations[0].file,
        PathBuf::from("src/renamed_a.rs")
    );
    assert_eq!(second_diagnostic.locations[0].line, Some(40));
    assert_eq!(second_diagnostic.locations[1].end_line, Some(95));
}

#[test]
fn missing_line_values_remain_unlocated() {
    let mut report = report();
    report
        .clone_violations
        .push(clone_violation("a.rs", (0, 0), "b.rs", (0, 0)));
    report
        .dead_code_violations
        .push(dead_code("Unused Export", None));

    let found = diagnostics(&report);
    assert_eq!(found[0].locations[0].line, None);
    assert_eq!(found[0].locations[0].end_line, None);
    assert_eq!(found[0].locations[1].line, None);
    assert_eq!(found[1].locations[0].line, None);
}
