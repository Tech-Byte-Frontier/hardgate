use super::{changed, coverage, coverage_map, strict_scorer};
use hardgate::engines::CoverageScorer;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[test]
fn test_diff_coverage_covered_and_uncovered_hunks() {
    let scorer = strict_scorer();
    let map = coverage_map("src/calc.rs", &[(1, 1), (2, 0), (3, 1)]);

    assert!(
        scorer
            .evaluate_diff_coverage(&map, &changed("src/calc.rs", &[1, 3]))
            .is_empty()
    );
    let violations = scorer.evaluate_diff_coverage(&map, &changed("src/calc.rs", &[1, 2]));
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].metric, "Diff Line Coverage");
    assert_eq!(violations[0].file, PathBuf::from("src/calc.rs"));
    assert_eq!(violations[0].actual, 50.0);
    assert_eq!(violations[0].limit, 100.0);
    assert!(violations[0].message.contains("2"));
}

#[test]
fn test_diff_coverage_matches_absolute_and_relative_paths() {
    let scorer = strict_scorer();
    let absolute = coverage_map("/repo/src/calc.rs", &[(7, 1)]);
    assert!(
        scorer
            .evaluate_diff_coverage(&absolute, &changed("src/calc.rs", &[7]))
            .is_empty()
    );

    let relative = coverage_map("src/calc.rs", &[(7, 1)]);
    assert!(
        scorer
            .evaluate_diff_coverage(&relative, &changed("/repo/src/calc.rs", &[7]))
            .is_empty()
    );
}

#[test]
fn test_diff_coverage_missing_changed_source_is_a_violation() {
    let scorer = strict_scorer();
    let violations =
        scorer.evaluate_diff_coverage(&HashMap::new(), &changed("src/missing.rs", &[4]));
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].metric, "Missing Diff Coverage");
    assert_eq!(violations[0].file, PathBuf::from("src/missing.rs"));
    assert!(violations[0].message.contains("src/missing.rs"));
}

#[test]
fn test_diff_coverage_ignores_non_executable_hunk() {
    let scorer = strict_scorer();
    let map = coverage_map("src/calc.rs", &[(2, 1)]);
    assert!(
        scorer
            .evaluate_diff_coverage(&map, &changed("src/calc.rs", &[1, 3]))
            .is_empty()
    );
}

#[test]
fn test_full_evaluation_remains_full_project_mode() {
    let scorer = strict_scorer();
    let map = coverage_map("src/calc.rs", &[(1, 1), (2, 0)]);
    let _ = scorer.evaluate_diff_coverage(&map, &changed("src/calc.rs", &[1, 2]));
    let full = scorer.evaluate(&map, &[], Path::new("."));
    assert!(full.iter().any(|v| v.metric == "Global Line Coverage"));
    assert!(full.iter().all(|v| v.metric != "Diff Line Coverage"));
}

#[test]
fn compatibility_diff_matching_never_uses_first_ambiguous_record() {
    let scorer = strict_scorer();
    let map = HashMap::from([
        (
            PathBuf::from("one/src/lib.rs"),
            coverage("one/src/lib.rs", &[(1, 1)]),
        ),
        (
            PathBuf::from("two/src/lib.rs"),
            coverage("two/src/lib.rs", &[(1, 1)]),
        ),
    ]);
    let violations = scorer.evaluate_diff_coverage(&map, &changed("src/lib.rs", &[1]));
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].metric, "Missing Diff Coverage");
}

#[test]
fn strict_diff_reports_code_lines_missing_from_da() {
    let config = super::verify_config();
    let scorer = CoverageScorer::new(&config.coverage);
    let map = coverage_map("src/lib.rs", &[(1, 1)]);
    let lines = changed("src/lib.rs", &[1, 2]);
    let violations = scorer.evaluate_diff_coverage_strict(&map, &lines, Path::new("."));
    assert!(
        violations
            .iter()
            .any(|v| { v.metric == "Missing Diff Coverage" && v.message.contains("2") })
    );
}
