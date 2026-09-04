#[path = "support/fs.rs"]
mod fs;

#[path = "support/metrics.rs"]
mod metrics;

use fs::tempdir;

use hardgate::commands::verify::{
    CoverageVerification, verify_coverage, verify_coverage_with_diff,
};
use hardgate::config::{CoverageConfig, HardgateConfig};
use hardgate::diagnostics::GateReport;
use hardgate::engines::CoverageScorer;
use hardgate::engines::coverage::FileCoverage;
use hardgate::git_evidence::ChangedLineMap;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

fn strict_scorer() -> CoverageScorer {
    CoverageScorer::new(&CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(90.0),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: Some(25.0),
        critical_paths: Some(vec!["src/calc.rs".to_string()]),
    })
}

fn coverage(path: &str, hits: &[(usize, usize)]) -> FileCoverage {
    let line_hits = hits.iter().copied().collect::<HashMap<_, _>>();
    FileCoverage {
        file_path: PathBuf::from(path),
        lines_found: hits.len(),
        lines_hit: hits.iter().filter(|(_, count)| *count > 0).count(),
        line_hits,
        ..Default::default()
    }
}

fn changed(path: &str, lines: &[usize]) -> BTreeMap<PathBuf, BTreeSet<usize>> {
    BTreeMap::from([(PathBuf::from(path), lines.iter().copied().collect())])
}

fn coverage_map(path: &str, hits: &[(usize, usize)]) -> HashMap<PathBuf, FileCoverage> {
    HashMap::from([(PathBuf::from(path), coverage(path, hits))])
}

fn verify_config() -> HardgateConfig {
    HardgateConfig {
        coverage: CoverageConfig {
            enabled: true,
            report: None,
            min_line_percent: Some(90.0),
            min_function_percent: None,
            min_branch_percent: None,
            max_crap_score: None,
            critical_paths: None,
        },
        ..HardgateConfig::default()
    }
}

fn write_verify_report(path: &Path) {
    std::fs::write(
        path,
        "SF:src/calc.rs\nDA:1,1\nDA:2,0\nLF:2\nLH:1\nend_of_record\n",
    )
    .unwrap();
}

fn run_diff_verification(
    config: &HardgateConfig,
    report_path: &Path,
    changed_lines: Option<&ChangedLineMap>,
    report: &mut GateReport,
) {
    verify_coverage_with_diff(CoverageVerification {
        config,
        cli_report: Some(report_path.display().to_string()),
        functions: &[],
        changed_lines,
        report,
    });
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

    // High complexity (10) and low coverage (0.2):
    // CRAP = 100 * (0.8)^3 + 10 = 61.2, well above a 25.0 ceiling.
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
fn test_lcov_checksum_and_paths() {
    let tmp = tempdir("lcov");
    let report = tmp.join("lcov.info");
    std::fs::write(
        &report,
        "SF:/repo/src/calc.rs\nDA:1,0,AAAAAAAA\nDA:2,0,BBBBBBBB\nDA:3,1\nLF:3\nLH:1\nend_of_record\n",
    )
    .unwrap();

    let scorer = strict_scorer();
    let map = scorer.parse_lcov(&report).unwrap();
    // The checksum suffix must not drop the line.
    let cov = map.values().next().unwrap();
    assert_eq!(cov.line_hits.get(&1), Some(&0));
    assert_eq!(cov.line_hits.get(&2), Some(&0));

    // An absolute report path must match a relative function path.
    let funcs = vec![metrics::sample_metrics(3, 10, 5.0, 5.0)];
    let violations = scorer.evaluate(&map, &funcs, Path::new("/repo"));
    assert!(violations.iter().any(|v| v.metric == "CRAP Score"));
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Critical Path 100% Coverage")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_lcov_rejects_empty_and_malformed_reports() {
    let tmp = tempdir("lcov-invalid");
    let scorer = strict_scorer();
    let empty = tmp.join("empty.info");
    std::fs::write(&empty, "").unwrap();
    assert!(scorer.parse_lcov(&empty).is_err());

    let malformed = tmp.join("malformed.info");
    std::fs::write(
        &malformed,
        "SF:src/calc.rs\nDA:not-a-line,wat\nend_of_record\n",
    )
    .unwrap();
    assert!(scorer.parse_lcov(&malformed).is_err());
    let _ = std::fs::remove_dir_all(tmp);
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
fn verify_diff_mode_ignores_global_floor_findings() {
    let tmp = tempdir("verify-diff-floor");
    let report_path = tmp.join("lcov.info");
    write_verify_report(&report_path);
    let changed = changed("src/calc.rs", &[1]);
    let mut report = GateReport::new("verify".to_string());

    let config = verify_config();
    run_diff_verification(&config, &report_path, Some(&changed), &mut report);

    assert!(report.coverage_violations.is_empty());
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn verify_diff_mode_reports_uncovered_and_missing_changed_sources() {
    let tmp = tempdir("verify-diff-violations");
    let report_path = tmp.join("lcov.info");
    write_verify_report(&report_path);
    let changed = BTreeMap::from([
        (PathBuf::from("src/calc.rs"), BTreeSet::from([2])),
        (PathBuf::from("src/missing.rs"), BTreeSet::from([4])),
    ]);
    let mut report = GateReport::new("verify".to_string());

    let config = verify_config();
    run_diff_verification(&config, &report_path, Some(&changed), &mut report);

    assert_eq!(report.coverage_violations.len(), 2);
    assert!(report.coverage_violations.iter().any(|violation| {
        violation.metric == "Diff Line Coverage"
            && violation.file == Path::new("src/calc.rs")
            && violation.actual == 0.0
            && violation.message.contains("2")
    }));
    assert!(report.coverage_violations.iter().any(|violation| {
        violation.metric == "Missing Diff Coverage" && violation.file == Path::new("src/missing.rs")
    }));
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn verify_coverage_wrapper_remains_full_project_mode() {
    let tmp = tempdir("verify-full");
    let report_path = tmp.join("lcov.info");
    write_verify_report(&report_path);
    let mut report = GateReport::new("verify".to_string());

    verify_coverage(
        &verify_config(),
        Some(report_path.display().to_string()),
        &[],
        &mut report,
    );

    assert_eq!(report.coverage_violations.len(), 1);
    assert_eq!(report.coverage_violations[0].metric, "Global Line Coverage");
    assert_eq!(report.coverage_violations[0].actual, 50.0);
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn verify_diff_mode_respects_disabled_policy() {
    let mut config = verify_config();
    config.coverage.enabled = false;
    let changed = changed("src/missing.rs", &[4]);
    let mut report = GateReport::new("verify".to_string());

    verify_coverage_with_diff(CoverageVerification {
        config: &config,
        cli_report: Some("missing-lcov.info".to_string()),
        functions: &[],
        changed_lines: Some(&changed),
        report: &mut report,
    });

    assert!(report.coverage_violations.is_empty());
    assert!(report.orchestration_violations.is_empty());
    assert!(report.advisories.is_empty());
}
