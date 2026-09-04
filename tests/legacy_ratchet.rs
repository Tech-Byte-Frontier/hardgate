#[path = "common/legacy.rs"]
mod legacy;
use legacy::changes;

use hardgate::GateReport;
use hardgate::adoption::apply_legacy_ratchet;
use hardgate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, CoverageViolation, DeadCodeViolation,
    InvariantViolation, MutationViolation, OrchestrationViolation, SuppressionViolation,
};
fn report() -> GateReport {
    GateReport::new("legacy".to_string())
}
fn budget(file: &str, metric: &str, actual: usize) -> BudgetViolation {
    BudgetViolation {
        file: file.into(),
        metric: metric.into(),
        actual,
        limit: 10,
        message: "budget debt".to_string(),
    }
}
fn suppression(file: &str, line_number: usize, line_content: &str) -> SuppressionViolation {
    SuppressionViolation {
        file: file.into(),
        line_number,
        token: "@ts-ignore".to_string(),
        line_content: line_content.to_string(),
        message: "suppression debt".to_string(),
    }
}
fn complexity(file: &str, line_number: usize, actual: f64) -> ComplexityViolation {
    ComplexityViolation {
        file: file.into(),
        function_name: "compute".to_string(),
        line_number,
        end_line: line_number,
        metric: "Cyclomatic Complexity".to_string(),
        actual,
        limit: 5.0,
        breakdown: Vec::new(),
        message: "complexity debt".to_string(),
        recommendation: "split".to_string(),
    }
}
fn invariant(file: &str, line_number: usize, line_content: &str) -> InvariantViolation {
    InvariantViolation {
        file: file.into(),
        line_number,
        rule_name: "boundaries".to_string(),
        violation_type: "Disallowed Import".to_string(),
        offending_target: "private/db".to_string(),
        line_content: line_content.to_string(),
        message: "invariant debt".to_string(),
    }
}
fn clone_violation(
    files: (&str, &str),
    ranges: ((usize, usize), (usize, usize)),
    fingerprint: &str,
) -> CloneViolation {
    let ((file_a, file_b), (lines_a, lines_b)) = (files, ranges);
    CloneViolation {
        file_a: file_a.into(),
        lines_a,
        file_b: file_b.into(),
        lines_b,
        tokens: 50,
        lines: 5,
        fingerprint: fingerprint.to_string(),
        message: "clone debt".to_string(),
        recommendation: "extract".to_string(),
    }
}
fn dead_code(file: &str, kind: &str, symbol: Option<&str>) -> DeadCodeViolation {
    DeadCodeViolation {
        file: file.into(),
        line_number: Some(1),
        symbol: symbol.map(str::to_string),
        violation_type: kind.to_string(),
        message: "dead-code debt".to_string(),
        recommendation: "remove".to_string(),
    }
}
fn debt_report() -> GateReport {
    let mut report = report();
    report
        .budget_violations
        .push(budget("src/a.rs", "lines", 100));
    report
        .complexity_violations
        .push(complexity("src/a.rs", 4, 10.0));
    report
        .suppression_violations
        .push(suppression("src/a.rs", 2, "// @ts-ignore"));
    report
        .invariant_violations
        .push(invariant("src/a.rs", 3, "use private/db;"));
    report.clone_violations.push(clone_violation(
        ("src/a.rs", "src/b.rs"),
        ((1, 5), (8, 12)),
        "fingerprint",
    ));
    report
        .dead_code_violations
        .push(dead_code("src/a.rs", "Unused Export", Some("old")));
    report
}
fn append_tail(report: &mut GateReport, line: usize, dead_file: &str, symbol: &str) {
    report
        .invariant_violations
        .push(invariant("src/a.rs", line, "use private/db;"));
    report.clone_violations.push(clone_violation(
        ("src/a.rs", "src/b.rs"),
        ((1, 3), (5, 6)),
        "fingerprint",
    ));
    report
        .dead_code_violations
        .push(dead_code(dead_file, "Unused Export", Some(symbol)));
}
fn push_core(
    report: &mut GateReport,
    budget_actual: usize,
    complexity_actual: f64,
    suppression_data: (usize, &str),
) {
    report
        .budget_violations
        .push(budget("src/a.rs", "lines", budget_actual));
    report
        .complexity_violations
        .push(complexity("src/a.rs", 4, complexity_actual));
    report.suppression_violations.push(suppression(
        "src/a.rs",
        suppression_data.0,
        suppression_data.1,
    ));
}
#[test]
fn unchanged_or_improved_static_debt_becomes_advisory() {
    let baseline = debt_report();

    let mut current = report();
    push_core(&mut current, 90, 10.0, (99, "  //   @ts-ignore  "));
    append_tail(&mut current, 30, "src/a.rs", "old");

    let outcome = apply_legacy_ratchet(&mut current, &baseline, &changes(&[], &[], &[]));

    assert_eq!(outcome.grandfathered, 6);
    assert_eq!(outcome.retained, 0);
    assert!(current.passed);
    assert!(current.budget_violations.is_empty());
    assert_eq!(current.advisories.len(), 6);
    assert!(
        outcome
            .advisories
            .iter()
            .all(|advisory| advisory.contains("abc123-merge-base"))
    );
}
#[test]
fn worsened_and_new_debt_remains_blocking() {
    let baseline = debt_report();

    let mut current = report();
    push_core(&mut current, 101, 11.0, (2, "// @ts-ignore changed"));
    current
        .budget_violations
        .push(budget("src/new.rs", "lines", 20));

    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(
            &[("src/a.rs", &[2, 4]), ("src/new.rs", &[1])],
            &["src/a.rs", "src/new.rs"],
            &[],
        ),
    );

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(outcome.retained, 4);
    assert!(!current.passed);
    assert!(
        current.budget_violations[0]
            .message
            .contains("changed file")
    );
    assert!(
        current.complexity_violations[0]
            .message
            .contains("changed hunk")
    );
}
#[test]
fn duplicate_multiset_finding_is_new_debt() {
    let mut baseline = report();
    baseline
        .suppression_violations
        .push(suppression("src/a.ts", 2, "// @ts-ignore"));
    let mut current = report();
    current
        .suppression_violations
        .push(suppression("src/a.ts", 2, "// @ts-ignore"));
    current
        .suppression_violations
        .push(suppression("src/a.ts", 99, "// @ts-ignore"));

    let outcome = apply_legacy_ratchet(&mut current, &baseline, &changes(&[], &[], &[]));

    assert_eq!(outcome.grandfathered, 1);
    assert_eq!(current.suppression_violations.len(), 1);
    assert!(!current.passed);
}
#[test]
fn rename_lineage_grandfathers_but_copied_path_does_not() {
    let mut baseline = report();
    baseline
        .dead_code_violations
        .push(dead_code("src/old.ts", "Unreferenced File", None));
    let mut renamed = report();
    renamed
        .dead_code_violations
        .push(dead_code("src/new.ts", "Unreferenced File", None));
    let renamed_outcome = apply_legacy_ratchet(
        &mut renamed,
        &baseline,
        &changes(&[], &[], &[("src/new.ts", "src/old.ts")]),
    );
    assert_eq!(renamed_outcome.grandfathered, 1);
    assert!(renamed.passed);

    let mut copied = report();
    copied
        .dead_code_violations
        .push(dead_code("src/copy.ts", "Unreferenced File", None));
    let copied_outcome = apply_legacy_ratchet(&mut copied, &baseline, &changes(&[], &[], &[]));
    assert_eq!(copied_outcome.grandfathered, 0);
    assert_eq!(copied.dead_code_violations.len(), 1);
    assert!(!copied.passed);
}
#[test]
fn clone_fingerprint_is_required_and_rename_stable() {
    let mut baseline = report();
    baseline.clone_violations.push(clone_violation(
        ("src/a.rs", "src/b.rs"),
        ((1, 5), (8, 12)),
        "fingerprint-1",
    ));
    let mut renamed = report();
    renamed.clone_violations.push(clone_violation(
        ("src/new-b.rs", "src/new-a.rs"),
        ((8, 12), (1, 5)),
        "fingerprint-1",
    ));
    let outcome = apply_legacy_ratchet(
        &mut renamed,
        &baseline,
        &changes(
            &[],
            &[],
            &[("src/new-a.rs", "src/a.rs"), ("src/new-b.rs", "src/b.rs")],
        ),
    );
    assert_eq!(outcome.grandfathered, 1);
    assert!(renamed.passed);

    let mut changed = report();
    changed.clone_violations.push(clone_violation(
        ("src/a.rs", "src/b.rs"),
        ((1, 5), (8, 12)),
        "fingerprint-2",
    ));
    apply_legacy_ratchet(&mut changed, &baseline, &changes(&[], &[], &[]));
    assert_eq!(changed.clone_violations.len(), 1);

    let mut legacy = report();
    legacy.clone_violations.push(clone_violation(
        ("src/a.rs", "src/b.rs"),
        ((1, 5), (8, 12)),
        "",
    ));
    let mut current = report();
    current.clone_violations.push(clone_violation(
        ("src/a.rs", "src/b.rs"),
        ((1, 5), (8, 12)),
        "",
    ));
    apply_legacy_ratchet(&mut current, &legacy, &changes(&[], &[], &[]));
    assert_eq!(current.clone_violations.len(), 1);
}
#[test]
fn deleted_baseline_debt_has_no_advisory_or_blocker() {
    let mut baseline = report();
    baseline
        .budget_violations
        .push(budget("src/deleted.rs", "lines", 100));
    let mut current = report();
    let outcome = apply_legacy_ratchet(&mut current, &baseline, &changes(&[], &[], &[]));
    assert_eq!(outcome.grandfathered, 0);
    assert!(outcome.advisories.is_empty());
    assert!(current.passed);
}
#[test]
fn evidence_categories_are_never_ratcheted() {
    let mut baseline = report();
    baseline.coverage_violations.push(CoverageViolation {
        file: "src/a.rs".into(),
        function_name: None,
        metric: "Global Line Coverage".into(),
        actual: 50.0,
        limit: 95.0,
        message: "coverage".into(),
        recommendation: "cover".into(),
    });
    let mut current = report();
    current
        .coverage_violations
        .push(baseline.coverage_violations[0].clone());
    current.mutation_violations.push(MutationViolation {
        report_file: "coverage.json".into(),
        metric: "Mutation Score".into(),
        actual: 50.0,
        limit: 85.0,
        message: "mutation".into(),
        recommendation: "mutate".into(),
    });
    current
        .orchestration_violations
        .push(OrchestrationViolation {
            step: "test".into(),
            command: "cargo test".into(),
            exit_code: Some(1),
            output: "failed".into(),
            recommendation: "fix".into(),
        });

    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(&[("src/a.rs", &[3])], &["src/a.rs"], &[]),
    );
    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(current.coverage_violations.len(), 1);
    assert_eq!(current.mutation_violations.len(), 1);
    assert_eq!(current.orchestration_violations.len(), 1);
    assert!(!current.passed);
    assert!(
        current.coverage_violations[0]
            .message
            .contains("changed file")
    );
}
#[test]
fn retained_findings_get_deterministic_hunk_context() {
    let mut current = report();
    current
        .budget_violations
        .push(budget("src/a.rs", "lines", 100));
    current
        .suppression_violations
        .push(suppression("src/a.rs", 2, "// @ts-ignore"));
    current
        .complexity_violations
        .push(complexity("src/a.rs", 4, 10.0));
    append_tail(&mut current, 6, "src/c.ts", "unused");

    apply_legacy_ratchet(
        &mut current,
        &report(),
        &changes(
            &[
                ("src/a.rs", &[1, 2, 3, 4, 6]),
                ("src/b.rs", &[5, 6]),
                ("src/c.ts", &[1]),
            ],
            &["src/a.rs", "src/b.rs", "src/c.ts"],
            &[],
        ),
    );
    assert!(current.budget_violations[0].message.contains("at line 1"));
    assert!(current.suppression_violations[0].message.contains(":1-4"));
    assert!(current.complexity_violations[0].message.contains(":1-4"));
    assert!(current.invariant_violations[0].message.contains(":6"));
    assert!(
        current.clone_violations[0]
            .message
            .contains("`src/a.rs`:1-3")
    );
    assert!(
        current.clone_violations[0]
            .message
            .contains("`src/b.rs`:5-6")
    );
    assert!(
        current.dead_code_violations[0]
            .message
            .contains("changed hunk `src/c.ts`:1")
    );
}
#[test]
fn advisory_order_is_deterministic() {
    let mut first = report();
    first
        .complexity_violations
        .push(complexity("src/z.rs", 1, 2.0));
    first.budget_violations.push(budget("src/a.rs", "lines", 2));
    let mut second = report();
    second
        .budget_violations
        .push(first.budget_violations[0].clone());
    second
        .complexity_violations
        .push(first.complexity_violations[0].clone());
    let baseline = first.clone();
    let c = changes(&[], &[], &[]);

    let first_outcome = apply_legacy_ratchet(&mut first, &baseline, &c);
    let second_outcome = apply_legacy_ratchet(&mut second, &baseline, &c);
    assert_eq!(first_outcome.advisories, second_outcome.advisories);
    assert_eq!(first_outcome.advisories, {
        let mut sorted = first_outcome.advisories.clone();
        sorted.sort();
        sorted
    });
}
#[test]
fn normalized_line_content_ignores_spacing_and_line_numbers() {
    let mut baseline = report();
    baseline
        .invariant_violations
        .push(invariant("src/a.rs", 2, "use private/db;"));
    let mut current = report();
    current
        .invariant_violations
        .push(invariant("src/a.rs", 99, "  use   private/db;  "));
    let outcome = apply_legacy_ratchet(&mut current, &baseline, &changes(&[], &[], &[]));
    assert_eq!(outcome.grandfathered, 1);
    assert!(current.passed);
}

#[test]
fn stable_numeric_keys_do_not_cross_metrics() {
    let mut baseline = report();
    baseline
        .budget_violations
        .push(budget("src/a.rs", "lines", 100));
    let mut current = report();
    current
        .budget_violations
        .push(budget("src/a.rs", "bytes", 1));
    apply_legacy_ratchet(&mut current, &baseline, &changes(&[], &[], &[]));
    assert_eq!(current.budget_violations.len(), 1);
}

#[test]
fn structured_identity_fields_survive_annotation() {
    let mut current = report();
    current
        .complexity_violations
        .push(complexity("src/a.rs", 2, 10.0));
    let before = current.complexity_violations[0].clone();
    apply_legacy_ratchet(
        &mut current,
        &report(),
        &changes(&[("src/a.rs", &[2])], &["src/a.rs"], &[]),
    );
    let after = &current.complexity_violations[0];
    assert_eq!(after.file, before.file);
    assert_eq!(after.function_name, before.function_name);
    assert_eq!(after.line_number, before.line_number);
    assert_eq!(after.end_line, before.end_line);
    assert_eq!(after.metric, before.metric);
    assert_eq!(after.actual, before.actual);
    assert_eq!(after.limit, before.limit);
}
