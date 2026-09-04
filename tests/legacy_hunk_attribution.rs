use hardgate::GateReport;
use hardgate::adoption::apply_legacy_ratchet;
use hardgate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, DeadCodeViolation, InvariantViolation,
    SuppressionViolation,
};
use hardgate::git_evidence::{ChangeSet, ChangedLineMap};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn changes(lines: &[(&str, &[usize])], files: &[&str], renames: &[(&str, &str)]) -> ChangeSet {
    let mut changed_lines = ChangedLineMap::new();
    for (path, numbers) in lines {
        changed_lines.insert(PathBuf::from(path), numbers.iter().copied().collect());
    }
    ChangeSet {
        merge_base: "abc123-merge-base".to_string(),
        changed_lines,
        changed_files: files.iter().map(PathBuf::from).collect(),
        rename_lineage: renames
            .iter()
            .map(|(current, baseline)| (PathBuf::from(current), PathBuf::from(baseline)))
            .collect(),
    }
}

fn report() -> GateReport {
    GateReport::new("legacy".to_string())
}

fn budget(file: &str, actual: usize) -> BudgetViolation {
    BudgetViolation {
        file: file.into(),
        metric: "lines".into(),
        actual,
        limit: 10,
        message: "budget debt".into(),
    }
}

fn suppression(file: &str, line_number: usize) -> SuppressionViolation {
    SuppressionViolation {
        file: file.into(),
        line_number,
        token: "@ts-ignore".into(),
        line_content: "// @ts-ignore".into(),
        message: "suppression debt".into(),
    }
}

fn complexity(file: &str, line_number: usize, actual: f64) -> ComplexityViolation {
    complexity_span(file, line_number, line_number, actual)
}

fn complexity_span(
    file: &str,
    line_number: usize,
    end_line: usize,
    actual: f64,
) -> ComplexityViolation {
    ComplexityViolation {
        file: file.into(),
        function_name: "compute".into(),
        line_number,
        end_line,
        metric: "Cyclomatic Complexity".into(),
        actual,
        limit: 5.0,
        breakdown: Vec::new(),
        message: "complexity debt".into(),
        recommendation: "split".into(),
    }
}

fn invariant(file: &str, line_number: usize) -> InvariantViolation {
    InvariantViolation {
        file: file.into(),
        line_number,
        rule_name: "boundaries".into(),
        violation_type: "Disallowed Import".into(),
        offending_target: "private/db".into(),
        line_content: "use private/db;".into(),
        message: "invariant debt".into(),
    }
}

fn clone_violation(
    file_a: &str,
    lines_a: (usize, usize),
    file_b: &str,
    lines_b: (usize, usize),
) -> CloneViolation {
    CloneViolation {
        file_a: file_a.into(),
        lines_a,
        file_b: file_b.into(),
        lines_b,
        tokens: 50,
        lines: 5,
        fingerprint: "fingerprint".into(),
        message: "clone debt".into(),
        recommendation: "extract".into(),
    }
}

fn dead_code(file: &str, line_number: Option<usize>) -> DeadCodeViolation {
    DeadCodeViolation {
        file: file.into(),
        line_number,
        symbol: Some("old".into()),
        violation_type: "Unused Export".into(),
        message: "dead-code debt".into(),
        recommendation: "remove".into(),
    }
}

fn unreferenced_file(file: &str) -> DeadCodeViolation {
    DeadCodeViolation {
        file: file.into(),
        line_number: Some(1),
        symbol: None,
        violation_type: "Unreferenced File".into(),
        message: "dead-code debt".into(),
        recommendation: "remove".into(),
    }
}

#[test]
fn changed_hunks_block_matching_debt_in_every_static_vector() {
    let mut baseline = report();
    baseline.budget_violations.push(budget("src/a.rs", 100));
    baseline
        .suppression_violations
        .push(suppression("src/a.rs", 2));
    baseline
        .complexity_violations
        .push(complexity("src/a.rs", 4, 10.0));
    baseline.invariant_violations.push(invariant("src/a.rs", 3));
    baseline
        .clone_violations
        .push(clone_violation("src/a.rs", (1, 5), "src/b.rs", (8, 12)));
    baseline
        .dead_code_violations
        .push(dead_code("src/a.rs", Some(1)));

    let mut current = baseline.clone();
    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(
            &[("src/a.rs", &[1, 2, 3, 4]), ("src/b.rs", &[10])],
            &["src/a.rs", "src/b.rs"],
            &[],
        ),
    );

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(outcome.retained, 6);
    assert!(!current.passed);
}

#[test]
fn unrelated_changed_file_does_not_block_untouched_equal_debt() {
    let mut baseline = report();
    baseline.budget_violations.push(budget("src/a.rs", 100));
    baseline
        .suppression_violations
        .push(suppression("src/a.rs", 2));
    baseline
        .complexity_violations
        .push(complexity("src/a.rs", 4, 10.0));
    baseline.invariant_violations.push(invariant("src/a.rs", 3));
    baseline
        .clone_violations
        .push(clone_violation("src/a.rs", (1, 5), "src/b.rs", (8, 12)));
    baseline
        .dead_code_violations
        .push(dead_code("src/a.rs", Some(1)));

    let mut current = baseline.clone();
    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(&[("src/other.rs", &[9])], &["src/other.rs"], &[]),
    );

    assert_eq!(outcome.grandfathered, 6);
    assert_eq!(outcome.retained, 0);
    assert!(current.passed);
}

#[test]
fn equal_debt_edited_on_its_line_remains_blocking() {
    let mut baseline = report();
    baseline
        .suppression_violations
        .push(suppression("src/a.rs", 2));
    baseline
        .complexity_violations
        .push(complexity("src/a.rs", 4, 10.0));
    baseline.invariant_violations.push(invariant("src/a.rs", 3));
    baseline
        .dead_code_violations
        .push(dead_code("src/a.rs", Some(1)));

    let mut current = baseline.clone();
    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(&[("src/a.rs", &[1, 2, 3, 4])], &["src/a.rs"], &[]),
    );

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(outcome.retained, 4);
    assert!(!current.passed);
}

#[test]
fn interior_function_edit_blocks_matching_complexity_debt() {
    let mut baseline = report();
    baseline
        .complexity_violations
        .push(complexity_span("src/a.rs", 4, 8, 10.0));
    let mut current = baseline.clone();
    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(&[("src/a.rs", &[6])], &["src/a.rs"], &[]),
    );

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(current.complexity_violations.len(), 1);
    assert!(!current.passed);
}

#[test]
fn legacy_complexity_without_end_line_falls_back_to_start_line() {
    let baseline_violation = complexity_span("src/a.rs", 4, 8, 10.0);
    let mut payload = serde_json::to_value(&baseline_violation).unwrap();
    payload.as_object_mut().unwrap().remove("end_line");
    let legacy: ComplexityViolation = serde_json::from_value(payload).unwrap();
    assert_eq!(legacy.end_line, 0);

    let mut baseline = report();
    baseline.complexity_violations.push(baseline_violation);
    let mut current = report();
    current.complexity_violations.push(legacy);
    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(&[("src/a.rs", &[4])], &["src/a.rs"], &[]),
    );

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(current.complexity_violations.len(), 1);
    assert!(!current.passed);
}

#[test]
fn clone_debt_blocks_when_either_current_range_is_touched() {
    let mut baseline = report();
    baseline
        .clone_violations
        .push(clone_violation("src/a.rs", (1, 5), "src/b.rs", (8, 12)));

    for (path, line) in [("src/a.rs", 3), ("src/b.rs", 10)] {
        let mut current = baseline.clone();
        let outcome = apply_legacy_ratchet(
            &mut current,
            &baseline,
            &changes(&[(path, &[line])], &[path], &[]),
        );
        assert_eq!(outcome.grandfathered, 0, "touched {path}");
        assert_eq!(current.clone_violations.len(), 1, "touched {path}");
    }
}

#[test]
fn line_less_dead_code_and_budget_block_on_renamed_file_without_hunks() {
    let mut baseline = report();
    baseline.budget_violations.push(budget("src/old.rs", 100));
    baseline
        .dead_code_violations
        .push(dead_code("src/old.rs", None));

    let mut current = report();
    current.budget_violations.push(budget("src/new.rs", 100));
    current
        .dead_code_violations
        .push(dead_code("src/new.rs", None));
    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(
            &[],
            &["src/new.rs", "src/old.rs"],
            &[("src/new.rs", "src/old.rs")],
        ),
    );

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(outcome.retained, 2);
    assert!(!current.passed);
}

#[test]
fn unreferenced_file_debt_blocks_when_any_file_line_changes() {
    let mut baseline = report();
    baseline
        .dead_code_violations
        .push(unreferenced_file("src/a.rs"));
    let mut current = baseline.clone();
    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(&[("src/a.rs", &[5])], &["src/a.rs"], &[]),
    );

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(current.dead_code_violations.len(), 1);
    assert!(!current.passed);
}

#[test]
fn symbol_dead_code_uses_its_exact_changed_line() {
    let mut baseline = report();
    baseline
        .dead_code_violations
        .push(dead_code("src/a.rs", Some(1)));
    let mut current = baseline.clone();
    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(&[("src/a.rs", &[5])], &["src/a.rs"], &[]),
    );

    assert_eq!(outcome.grandfathered, 1);
    assert!(current.passed);
}

#[test]
fn pure_rename_without_hunks_preserves_line_and_clone_grandfathering() {
    let mut baseline = report();
    baseline
        .suppression_violations
        .push(suppression("src/old.rs", 2));
    baseline
        .complexity_violations
        .push(complexity("src/old.rs", 4, 10.0));
    baseline
        .invariant_violations
        .push(invariant("src/old.rs", 3));
    baseline
        .dead_code_violations
        .push(dead_code("src/old.rs", Some(1)));
    baseline.clone_violations.push(clone_violation(
        "src/old.rs",
        (1, 5),
        "src/other.rs",
        (8, 12),
    ));

    let mut current = report();
    current
        .suppression_violations
        .push(suppression("src/new.rs", 2));
    current
        .complexity_violations
        .push(complexity("src/new.rs", 4, 10.0));
    current
        .invariant_violations
        .push(invariant("src/new.rs", 3));
    current
        .dead_code_violations
        .push(dead_code("src/new.rs", Some(1)));
    current.clone_violations.push(clone_violation(
        "src/new.rs",
        (1, 5),
        "src/other-new.rs",
        (8, 12),
    ));
    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(
            &[],
            &[
                "src/new.rs",
                "src/old.rs",
                "src/other-new.rs",
                "src/other.rs",
            ],
            &[
                ("src/new.rs", "src/old.rs"),
                ("src/other-new.rs", "src/other.rs"),
            ],
        ),
    );

    assert_eq!(outcome.grandfathered, 5);
    assert_eq!(outcome.retained, 0);
    assert!(current.passed);
}

#[test]
fn deleted_baseline_and_new_current_debt_are_not_matched() {
    let mut baseline = report();
    baseline
        .suppression_violations
        .push(suppression("src/deleted.rs", 2));
    let mut current = report();
    current
        .suppression_violations
        .push(suppression("src/new.rs", 2));

    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(&[("src/new.rs", &[2])], &["src/new.rs"], &[]),
    );

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(outcome.retained, 1);
    assert!(!current.passed);
}

#[test]
fn changed_multiset_finding_does_not_consume_untouched_baseline_debt() {
    let mut baseline = report();
    baseline
        .suppression_violations
        .push(suppression("src/a.rs", 2));
    let mut current = report();
    current
        .suppression_violations
        .push(suppression("src/a.rs", 2));
    current
        .suppression_violations
        .push(suppression("src/a.rs", 2));

    let outcome = apply_legacy_ratchet(
        &mut current,
        &baseline,
        &changes(&[("src/a.rs", &[2])], &["src/a.rs"], &[]),
    );

    assert_eq!(outcome.grandfathered, 0);
    assert_eq!(current.suppression_violations.len(), 2);
    assert!(!current.passed);
}

#[test]
fn empty_changed_line_set_is_not_a_hunk_for_line_findings() {
    let mut baseline = report();
    baseline
        .suppression_violations
        .push(suppression("src/a.rs", 2));
    let mut current = baseline.clone();
    let mut change = changes(&[], &["src/a.rs"], &[]);
    change
        .changed_lines
        .insert(PathBuf::from("src/a.rs"), BTreeSet::new());

    let outcome = apply_legacy_ratchet(&mut current, &baseline, &change);

    assert_eq!(outcome.grandfathered, 1);
    assert!(current.passed);
}
