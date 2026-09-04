use super::{coverage, fs, metrics, source_scope_violations, strict_scorer};
use hardgate::config::CoverageConfig;
use hardgate::engines::CoverageScorer;
use hardgate::engines::coverage::FileCoverage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[test]
fn test_crap_score_calculation() {
    let scorer = CoverageScorer::new(&CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(80.0),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: Some(25.0),
        critical_paths: None,
    });

    let mut cov_map = HashMap::new();
    let mut file_cov = FileCoverage {
        file_path: PathBuf::from("src/calc.rs"),
        lines_found: 10,
        lines_hit: 2,
        ..Default::default()
    };
    for line in 1..=10 {
        file_cov
            .line_hits
            .insert(line, if line <= 2 { 1 } else { 0 });
    }
    cov_map.insert(file_cov.file_path.clone(), file_cov);

    let funcs = vec![metrics::sample_metrics(10, 12, 20.0, 12.0)];
    let violations = scorer.evaluate(&cov_map, &funcs, Path::new("."));
    let crap = violations.iter().find(|v| v.metric == "CRAP Score");
    assert!(matches!(crap, Some(v) if v.actual > 25.0));
}

#[test]
fn test_missing_critical_path_is_a_violation() {
    let scorer = strict_scorer();
    let mut map = HashMap::new();
    map.insert(
        PathBuf::from("src/other.rs"),
        FileCoverage {
            file_path: PathBuf::from("src/other.rs"),
            lines_found: 1,
            lines_hit: 1,
            ..Default::default()
        },
    );
    let violations = scorer.evaluate(&map, &[], Path::new("."));
    assert!(
        violations
            .iter()
            .any(|violation| violation.metric == "Missing Critical Path")
    );
}

#[test]
fn zero_denominators_are_blocking_for_every_enabled_global_floor() {
    let config = CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(1.0),
        min_function_percent: Some(1.0),
        min_branch_percent: Some(1.0),
        max_crap_score: None,
        critical_paths: None,
    };
    let scorer = CoverageScorer::new(&config);
    let map = HashMap::from([(
        PathBuf::from("src/lib.rs"),
        FileCoverage {
            file_path: PathBuf::from("src/lib.rs"),
            ..Default::default()
        },
    )]);
    let source = [PathBuf::from("src/lib.rs")];
    let violations = scorer.evaluate_for_sources(
        &map,
        &[],
        super::CoverageEvaluationScope {
            root: Path::new("."),
            source_files: Some(&source),
        },
    );
    for metric in [
        "Global Line Coverage",
        "Global Function Coverage",
        "Global Branch Coverage",
    ] {
        assert!(
            violations
                .iter()
                .any(|v| v.metric == metric && v.actual == 0.0)
        );
    }
}

#[test]
fn hostile_counter_addition_is_reported_without_wrapping() {
    let config = CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(1.0),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: None,
        critical_paths: None,
    };
    let scorer = CoverageScorer::new(&config);
    let max = usize::MAX;
    let map = HashMap::from([
        (
            PathBuf::from("src/one.rs"),
            FileCoverage {
                file_path: PathBuf::from("src/one.rs"),
                lines_found: max,
                ..Default::default()
            },
        ),
        (
            PathBuf::from("src/two.rs"),
            FileCoverage {
                file_path: PathBuf::from("src/two.rs"),
                lines_found: 1,
                ..Default::default()
            },
        ),
    ]);
    let sources = [PathBuf::from("src/one.rs"), PathBuf::from("src/two.rs")];
    let violations = scorer.evaluate_for_sources(
        &map,
        &[],
        super::CoverageEvaluationScope {
            root: Path::new("."),
            source_files: Some(&sources),
        },
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.metric == "Coverage Count Overflow")
    );
}

#[test]
fn test_and_generated_records_cannot_dilute_source_floor() {
    let map = HashMap::from([
        (
            PathBuf::from("src/lib.rs"),
            coverage("src/lib.rs", &[(1, 0)]),
        ),
        (
            PathBuf::from("tests/lib.rs"),
            coverage("tests/lib.rs", &[(1, 1), (2, 1), (3, 1)]),
        ),
    ]);
    let source = [PathBuf::from("src/lib.rs")];
    let violations = source_scope_violations(90.0, &map, Path::new("."), &source);
    let global = violations
        .iter()
        .find(|violation| violation.metric == "Global Line Coverage")
        .expect("source line floor should fail");
    assert_eq!(global.actual, 0.0);
}

#[test]
fn duplicate_normalized_report_paths_are_ambiguous() {
    let map = HashMap::from([
        (
            PathBuf::from("/repo/src/lib.rs"),
            coverage("/repo/src/lib.rs", &[(1, 1)]),
        ),
        (
            PathBuf::from("src/lib.rs"),
            coverage("src/lib.rs", &[(1, 1)]),
        ),
    ]);
    let source = [PathBuf::from("src/lib.rs")];
    let violations = source_scope_violations(1.0, &map, Path::new("/repo"), &source);
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Missing Source Coverage")
    );
}

#[test]
fn scoped_source_matching_rejects_suffix_only_paths() {
    let map = super::coverage_map("packages/other/src/lib.rs", &[(1, 1)]);
    let source = [PathBuf::from("src/lib.rs")];
    let violations = source_scope_violations(1.0, &map, Path::new("."), &source);
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Missing Source Coverage")
    );
}

#[test]
fn absolute_report_paths_under_root_match_exact_sources() {
    let root = fs::tempdir("coverage-absolute");
    let source = root.join("src/lib.rs");
    let map = super::coverage_map(&source.display().to_string(), &[(1, 1)]);
    let violations = source_scope_violations(90.0, &map, &root, std::slice::from_ref(&source));
    assert!(
        violations
            .iter()
            .all(|violation| violation.metric != "Missing Source Coverage")
    );
    let _ = std::fs::remove_dir_all(root);
}
