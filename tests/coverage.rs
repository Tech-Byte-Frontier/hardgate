#[path = "support/fs.rs"]
mod fs;

#[path = "support/metrics.rs"]
mod metrics;

use fs::tempdir;

use hardgate::config::CoverageConfig;
use hardgate::engines::CoverageScorer;
use hardgate::engines::coverage::FileCoverage;
use std::collections::HashMap;
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
