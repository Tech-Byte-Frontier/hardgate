use super::{coverage, fs, metrics, source_scope_violations, strict_scorer};
use hardgate::config::CoverageConfig;
use hardgate::engines::complexity::FunctionMetrics;
use hardgate::engines::coverage::FileCoverage;
use hardgate::engines::{CoverageScorer, CoverageViolation};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn function_metric(
    file: &str,
    start_line: usize,
    end_line: usize,
    cyclomatic: u32,
) -> FunctionMetrics {
    FunctionMetrics {
        name: "fixture_function".to_string(),
        file: PathBuf::from(file),
        start_line,
        end_line,
        lines: end_line.saturating_sub(start_line).saturating_add(1),
        parameters: 0,
        cyclomatic,
        cognitive: 0,
        halstead_difficulty: 0.0,
        max_nesting_depth: 0,
        statements: 0,
        abc_score: 0.0,
        cognitive_breakdown: Vec::new(),
        cyclomatic_breakdown: Vec::new(),
    }
}

fn scoring_config(
    min_line_percent: Option<f64>,
    min_function_percent: Option<f64>,
    min_branch_percent: Option<f64>,
    max_crap_score: Option<f64>,
) -> CoverageConfig {
    CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent,
        min_function_percent,
        min_branch_percent,
        max_crap_score,
        critical_paths: None,
    }
}

fn coverage_records(entries: &[(&str, &[(usize, usize)])]) -> HashMap<PathBuf, FileCoverage> {
    let mut map = HashMap::new();
    for &(path, hits) in entries {
        map.insert(PathBuf::from(path), coverage(path, hits));
    }
    map
}

fn has_missing_source(violations: &[CoverageViolation]) -> bool {
    violations
        .iter()
        .any(|violation| violation.metric == "Missing Source Coverage")
}

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
fn crap_scoring_skips_cfg_excluded_functions_but_scores_zero_hit_functions() {
    let config = scoring_config(None, None, None, Some(25.0));
    let scorer = CoverageScorer::new(&config);
    let function = function_metric("src/platform.rs", 10, 20, 7);

    let excluded = coverage_records(&[("src/platform.rs", &[(1, 1)])]);
    let violations = scorer.evaluate(&excluded, std::slice::from_ref(&function), Path::new("."));
    assert!(
        violations
            .iter()
            .all(|violation| violation.metric != "CRAP Score")
    );

    let uncovered = coverage_records(&[("src/platform.rs", &[(10, 0)])]);
    let violations = scorer.evaluate(&uncovered, &[function], Path::new("."));
    assert!(
        violations
            .iter()
            .any(|violation| { violation.metric == "CRAP Score" && violation.actual == 56.0 })
    );
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
    let mut map = super::coverage_map("src/lib.rs", &[(1, 0)]);
    map.insert(
        PathBuf::from("tests/lib.rs"),
        coverage("tests/lib.rs", &[(1, 1), (2, 1), (3, 1)]),
    );
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
    let mut map = super::coverage_map("/repo/src/lib.rs", &[(1, 1)]);
    map.insert(
        PathBuf::from("src/lib.rs"),
        coverage("src/lib.rs", &[(1, 1)]),
    );
    let source = [PathBuf::from("src/lib.rs")];
    let violations = source_scope_violations(1.0, &map, Path::new("/repo"), &source);
    assert!(has_missing_source(&violations));
}

#[test]
fn scoped_source_matching_rejects_suffix_only_paths() {
    let map = super::coverage_map("packages/other/src/lib.rs", &[(1, 1)]);
    let source = [PathBuf::from("src/lib.rs")];
    let violations = source_scope_violations(1.0, &map, Path::new("."), &source);
    assert!(has_missing_source(&violations));
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

#[test]
fn full_scoring_deduplicates_missing_function_files() {
    let config = scoring_config(Some(1.0), None, None, None);
    let scorer = CoverageScorer::new(&config);
    let functions = vec![
        function_metric("src/missing.rs", 1, 1, 1),
        function_metric("src/missing.rs", 2, 2, 1),
        function_metric("src/other.rs", 1, 1, 1),
    ];
    let violations = scorer.evaluate(&HashMap::new(), &functions, Path::new("."));
    let missing = violations
        .iter()
        .filter(|violation| violation.metric == "Missing Source Coverage")
        .count();
    assert_eq!(missing, 2);
}

#[test]
fn scoped_scoring_deduplicates_sources_and_rejects_outside_root() {
    let map = super::coverage_map("src/lib.rs", &[(1, 1)]);
    let source = [
        PathBuf::from("src/lib.rs"),
        PathBuf::from("src/lib.rs"),
        PathBuf::from("."),
    ];
    let violations = source_scope_violations(1.0, &map, Path::new("."), &source);
    assert_eq!(
        violations
            .iter()
            .filter(|violation| violation.metric == "Missing Source Coverage")
            .count(),
        1
    );
    assert!(violations.iter().any(|violation| {
        violation.file == Path::new(".") && violation.message.contains("outside the repository")
    }));
}

#[test]
fn scoped_scoring_rejects_absolute_records_outside_root() {
    let root = fs::tempdir("coverage-outside-root");
    let map = super::coverage_map("/other/src/lib.rs", &[(1, 1)]);
    let source = [PathBuf::from("/other/src/lib.rs")];
    let violations = source_scope_violations(1.0, &map, &root, &source);
    assert!(violations.iter().any(|violation| {
        violation.metric == "Missing Source Coverage"
            && violation.message.contains("outside the repository")
    }));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn scoped_scoring_normalizes_windows_separators_under_root() {
    let root = fs::tempdir("coverage-windows-path");
    let report_path = format!("{}\\src\\lib.rs", root.display());
    let map = super::coverage_map(&report_path, &[(1, 1)]);
    let source = [root.join("src/lib.rs")];
    let violations = source_scope_violations(1.0, &map, &root, &source);
    assert!(
        violations
            .iter()
            .all(|violation| { violation.metric != "Missing Source Coverage" })
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn full_scoring_uses_only_unique_normalized_records() {
    let config = scoring_config(Some(100.0), None, None, None);
    let map = coverage_records(&[
        ("/repo/src/lib.rs", &[(1, 1)]),
        ("src/lib.rs", &[(1, 0)]),
        ("src/other.rs", &[(1, 1)]),
    ]);
    let violations = CoverageScorer::new(&config).evaluate(&map, &[], Path::new("/repo"));
    assert!(
        violations
            .iter()
            .all(|violation| violation.metric != "Global Line Coverage")
    );
}

#[test]
fn scoring_covers_passing_floors_and_full_critical_path() {
    let mut config = scoring_config(Some(100.0), Some(100.0), Some(100.0), Some(25.0));
    config.critical_paths = Some(vec!["src/full.rs".to_string()]);
    let mut file = coverage("src/full.rs", &[(1, 1)]);
    file.functions_found = 1;
    file.functions_hit = 1;
    file.branches_found = 1;
    file.branches_hit = 1;
    let map = HashMap::from([(file.file_path.clone(), file)]);
    let functions = [function_metric("src/full.rs", 1, 1, 1)];
    let violations = CoverageScorer::new(&config).evaluate(&map, &functions, Path::new("."));
    assert!(violations.is_empty());
}

#[test]
fn scoring_skips_out_of_scope_and_inverted_crap_ranges() {
    let config = scoring_config(None, None, None, Some(25.0));
    let map = coverage_records(&[
        ("src/in_scope.rs", &[(1, 1)]),
        ("src/out_of_scope.rs", &[(1, 0)]),
    ]);
    let source = [PathBuf::from("src/in_scope.rs")];
    let functions = [
        function_metric("src/in_scope.rs", 2, 1, 1),
        function_metric("src/out_of_scope.rs", 1, 1, 10),
    ];
    let violations = CoverageScorer::new(&config).evaluate_for_sources(
        &map,
        &functions,
        super::CoverageEvaluationScope {
            root: Path::new("."),
            source_files: Some(&source),
        },
    );
    assert!(violations.iter().all(|violation| {
        violation.metric != "CRAP Score" || violation.file == Path::new("src/in_scope.rs")
    }));
}
