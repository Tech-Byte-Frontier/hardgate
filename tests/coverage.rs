#[path = "support/fs.rs"]
mod fs;

#[path = "support/metrics.rs"]
mod metrics;

use hardgate::commands::verify::{
    CoverageVerification, verify_coverage, verify_coverage_with_diff, verify_coverage_with_scope,
};
use hardgate::config::{CoverageConfig, HardgateConfig};
use hardgate::diagnostics::GateReport;
use hardgate::engines::coverage::{CoverageEvaluationScope, FileCoverage};
use hardgate::engines::{CoverageScorer, CoverageViolation};
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

fn source_scope_config(min_line_percent: f64) -> CoverageConfig {
    CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(min_line_percent),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: None,
        critical_paths: None,
    }
}

fn source_scope_violations(
    min_line_percent: f64,
    map: &HashMap<PathBuf, FileCoverage>,
    root: &Path,
    source_files: &[PathBuf],
) -> Vec<CoverageViolation> {
    CoverageScorer::new(&source_scope_config(min_line_percent)).evaluate_for_sources(
        map,
        &[],
        CoverageEvaluationScope {
            root,
            source_files: Some(source_files),
        },
    )
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

fn assert_invalid_lcov(config: &CoverageConfig, body: &str) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let tag = format!(
        "lcov-adversarial-{}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let tmp = fs::tempdir(&tag);
    let path = tmp.join("report.info");
    std::fs::write(&path, body).unwrap();
    assert!(
        CoverageScorer::new(config).parse_lcov(&path).is_err(),
        "LCOV unexpectedly parsed: {body}"
    );
    let _ = std::fs::remove_dir_all(tmp);
}

fn detail_report(details: &str, counts: &str) -> String {
    format!(
        "# generated metadata\nTN:\nSF:src/lib.rs\nVER:1.0\n{details}DA:1,1\nLF:1\nLH:1\n{counts}\nend_of_record\n"
    )
}

#[path = "coverage/diff.rs"]
mod diff;
#[path = "coverage/lcov.rs"]
mod lcov;
#[path = "coverage/scoring.rs"]
mod scoring;
#[path = "coverage/verification.rs"]
mod verification;
