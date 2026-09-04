use super::{fs, run_diff_verification, verify_config, write_verify_report};
use hardgate::commands::verify::{CoverageScope, CoverageVerification};
use hardgate::diagnostics::GateReport;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[test]
fn verify_diff_mode_ignores_global_floor_findings() {
    let tmp = fs::tempdir("verify-diff-floor");
    let report_path = tmp.join("lcov.info");
    write_verify_report(&report_path);
    let changed = super::changed("src/calc.rs", &[1]);
    let mut report = GateReport::new("verify".to_string());

    let config = verify_config();
    run_diff_verification(&config, &report_path, Some(&changed), &mut report);

    assert!(report.coverage_violations.is_empty());
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn verify_diff_mode_reports_uncovered_and_missing_changed_sources() {
    let tmp = fs::tempdir("verify-diff-violations");
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
    let tmp = fs::tempdir("verify-full");
    let report_path = tmp.join("lcov.info");
    write_verify_report(&report_path);
    let mut report = GateReport::new("verify".to_string());

    super::verify_coverage(
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
    let changed = super::changed("src/missing.rs", &[4]);
    let mut report = GateReport::new("verify".to_string());

    super::verify_coverage_with_diff(CoverageVerification {
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

#[test]
fn scoped_verify_keeps_source_inventory_and_root() {
    let tmp = fs::tempdir("verify-scoped");
    let report_path = tmp.join("lcov.info");
    write_verify_report(&report_path);
    let source = vec![PathBuf::from("src/calc.rs")];
    let mut report = GateReport::new("verify".to_string());
    super::verify_coverage_with_scope(
        CoverageVerification {
            config: &verify_config(),
            cli_report: Some(report_path.display().to_string()),
            functions: &[],
            changed_lines: None,
            report: &mut report,
        },
        CoverageScope {
            source_files: &source,
            root: Path::new("."),
        },
    );
    assert!(
        report
            .coverage_violations
            .iter()
            .any(|violation| violation.metric == "Global Line Coverage")
    );
    let _ = std::fs::remove_dir_all(tmp);
}
