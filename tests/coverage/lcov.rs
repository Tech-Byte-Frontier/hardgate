use super::{assert_invalid_lcov, detail_report, fs, metrics, strict_scorer};
use hardgate::config::CoverageConfig;
use hardgate::engines::CoverageScorer;
use hardgate::engines::coverage::FileCoverage;
use std::collections::HashMap;
use std::path::Path;

fn detail_config(
    min_function_percent: Option<f64>,
    min_branch_percent: Option<f64>,
) -> CoverageConfig {
    CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(1.0),
        min_function_percent,
        min_branch_percent,
        max_crap_score: None,
        critical_paths: None,
    }
}

fn parse_valid_lcov(
    config: &CoverageConfig,
    body: &str,
    tag: &str,
) -> HashMap<std::path::PathBuf, FileCoverage> {
    let tmp = fs::tempdir(tag);
    let report = tmp.join("report.info");
    std::fs::write(&report, body).unwrap();
    let parsed = CoverageScorer::new(config).parse_lcov(&report).unwrap();
    let _ = std::fs::remove_dir_all(tmp);
    parsed
}

#[test]
fn test_lcov_checksum_and_paths() {
    let tmp = fs::tempdir("lcov");
    let report = tmp.join("lcov.info");
    std::fs::write(
        &report,
        "SF:/repo/src/calc.rs\nDA:1,0,AAAAAAAA\nDA:2,0,BBBBBBBB\nDA:3,1\nLF:3\nLH:1\nend_of_record\n",
    )
    .unwrap();

    let scorer = strict_scorer();
    let map = scorer.parse_lcov(&report).unwrap();
    let cov = map.values().next().unwrap();
    assert_eq!(cov.line_hits.get(&1), Some(&0));
    assert_eq!(cov.line_hits.get(&2), Some(&0));

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
    let tmp = fs::tempdir("lcov-invalid");
    let scorer = strict_scorer();
    let empty = tmp.join("empty.info");
    std::fs::write(&empty, "").unwrap();
    assert!(scorer.parse_lcov(&empty).is_err());
    assert!(scorer.parse_lcov(&tmp.join("missing.info")).is_err());

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
fn lcov_parser_accepts_metadata_and_consistent_details() {
    let config = detail_config(Some(1.0), Some(1.0));
    let body = detail_report(
        "FN:1,2,compute\nFNDA:1,compute\nBRDA:1,0,case,with,comma,1\n",
        "FNF:1\nFNH:1\nBRF:1\nBRH:1",
    );
    let parsed = parse_valid_lcov(&config, &body, "lcov-details-valid");
    assert_eq!(parsed.len(), 1);
}

#[test]
fn lcov_parser_accepts_llvm22_summary_supersets() {
    let config = detail_config(Some(1.0), None);
    let body = "# generated metadata\nTN:\nSF:src/lib.rs\nVER:1.0\n".to_string()
        + "FN:1,first\nFN:2,second\nFN:3,third\n"
        + "FNDA:1,first\nFNDA:0,second\nFNDA:2,third\n"
        + "DA:1,1\nDA:2,1\nLF:3\nLH:1\n"
        + "FNF:2\nFNH:1\nend_of_record\n";
    let tmp = fs::tempdir("lcov-llvm22-superset");
    let report = tmp.join("report.info");
    std::fs::write(&report, body).unwrap();
    let parsed = CoverageScorer::new(&config).parse_lcov(&report).unwrap();
    assert_eq!(parsed.len(), 1);
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn lcov_parser_accepts_comments_checksums_and_summary_only_counters() {
    let config = detail_config(Some(1.0), None);
    let body = "# outside comment\nTN:\nSF: src/lib.rs \nVER:1.0\n# inside comment\n".to_string()
        + "DA:1,1,checksum\nLF:1\nLH:1\nFNF:2\nFNH:1\n"
        + "end_of_record\n";
    let parsed = parse_valid_lcov(&config, &body, "lcov-valid-metadata");
    let coverage = parsed.get(&std::path::PathBuf::from("src/lib.rs")).unwrap();
    assert_eq!(coverage.functions_found, 2);
    assert_eq!(coverage.branches_hit, 0);
}

#[test]
fn lcov_percentages_handle_empty_and_populated_counters() {
    let empty = FileCoverage::default();
    assert_eq!(empty.line_coverage_percent(), 0.0);
    assert_eq!(empty.function_coverage_percent(), 0.0);
    assert_eq!(empty.branch_coverage_percent(), 0.0);

    let populated = FileCoverage {
        lines_found: 4,
        lines_hit: 2,
        functions_found: 3,
        functions_hit: 1,
        branches_found: 8,
        branches_hit: 6,
        ..Default::default()
    };
    assert_eq!(populated.line_coverage_percent(), 50.0);
    assert!((populated.function_coverage_percent() - 100.0 / 3.0).abs() < f64::EPSILON);
    assert_eq!(populated.branch_coverage_percent(), 75.0);
}

#[test]
fn lcov_parser_accepts_not_taken_branch_details() {
    let config = detail_config(None, Some(1.0));
    let body = detail_report("BRDA:1,0,0,-\n", "BRF:1\nBRH:0");
    let parsed = parse_valid_lcov(&config, &body, "lcov-branch-not-taken");
    assert_eq!(parsed.len(), 1);
}

#[test]
fn lcov_parser_rejects_marker_path_and_version_edges() {
    let config = detail_config(None, None);
    for body in [
        "SF\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "end_of_record:extra\n",
        "end_of_record\n",
        "TN\n",
        "TN:\n",
        "VER\n",
        "VER:1.0\n",
        "SF:\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src\0lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nVER:\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nVER:1.0\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nVER:1.0\nVER:2.0\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nSF:other.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    ] {
        assert_invalid_lcov(&config, body);
    }
}

#[test]
fn lcov_parser_rejects_function_detail_mismatches_and_duplicates() {
    let config = detail_config(None, None);
    for details in [
        "FN:1,compute\nFNF:1\nFNH:0\n",
        "FNDA:1,compute\nFNF:1\nFNH:1\n",
        "FN:1,compute\nFN:2,compute\nFNDA:1,compute\nFNF:1\nFNH:1\n",
        "FN:1,compute\nFNDA:1,other\nFNF:1\nFNH:1\n",
        "FN:1,\nFNF:1\nFNH:0\n",
        "FN:1,2,compute\nFNDA:1,compute\nFNF:2\nFNH:1\n",
    ] {
        assert_invalid_lcov(&config, &detail_report(details, ""));
    }
}

#[test]
fn lcov_parser_rejects_function_ranges_names_and_duplicate_hits() {
    let config = detail_config(None, None);
    for details in [
        "FN:7,5,backwards\nFNDA:0,backwards\n",
        "FN:not-a-line,compute\nFNDA:0,compute\n",
        "FN:1,2,\0\nFNDA:0,\0\n",
        "FN:1,compute\nFNDA:not-a-count,compute\n",
        "FN:1,compute\nFNDA:0,compute\nFNDA:1,compute\n",
    ] {
        assert_invalid_lcov(&config, &detail_report(details, ""));
    }
}

#[test]
fn lcov_parser_rejects_branch_detail_mismatches_and_malformed_fields() {
    let config = detail_config(None, None);
    for details in [
        "BRDA:1,0,0,1\n",
        "BRDA:1,0,0,1\nBRDA:1,0,0,0\n",
        "BRDA:0,0,0,1\n",
        "BRDA:1,-,0,1\n",
        "BRDA:1,0,-,1\n",
        "BRDA:1,0,0,unknown\n",
        "BRDA:1,0,0\n",
    ] {
        assert_invalid_lcov(&config, &detail_report(details, ""));
    }
    assert_invalid_lcov(&config, &detail_report("BRDA:1,0,0,1\n", "BRF:2\nBRH:1"));
}

#[test]
fn lcov_parser_rejects_branch_nuls_and_duplicate_identities() {
    let config = detail_config(None, None);
    for details in [
        "BRDA:1,0,0,1\nBRDA:1,0,0,2\n",
        "BRDA:1,0,0,\0\n",
        "BRDA:1,0,\0,1\n",
        "BRDA:1,0,0,\n",
    ] {
        assert_invalid_lcov(&config, &detail_report(details, ""));
    }
}

#[test]
fn lcov_parser_rejects_unknown_records_and_invalid_metadata_placement() {
    let config = detail_config(None, None);
    for body in [
        "JUNK:1\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "DA\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "TN\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "TN:\nSF:src/lib.rs\nTN:\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "VER:1.0\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nVER:1.0\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nFNL:1,compute\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nJUNK\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    ] {
        assert_invalid_lcov(&config, body);
    }
}

#[test]
fn lcov_parser_rejects_unbounded_records_and_inconsistent_counts() {
    let config = detail_config(None, None);
    for body in [
        "LF:1\n",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\n",
        "SF:src/lib.rs\nLF:0\nLH:0\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:0,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1,checksum,extra\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,,checksum\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,not-a-count\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nDA:2,0\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,0\nLF:2\nLH:3\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:0\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:./src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/./lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/../src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    ] {
        assert_invalid_lcov(&config, body);
    }
}

#[test]
fn lcov_parser_requires_paired_function_and_branch_counts_when_floors_enabled() {
    let config = detail_config(Some(1.0), Some(1.0));
    assert_invalid_lcov(
        &config,
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nFNF:1\nFNH:2\nBRF:1\nBRH:1\nend_of_record\n",
    );
    assert_invalid_lcov(
        &config,
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nFNF:1\nFNH:1\nBRF:1\nend_of_record\n",
    );
    assert_invalid_lcov(
        &config,
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nFNF:1\nFNH:1\nBRF:1\nBRH:2\nend_of_record\n",
    );
}

#[test]
fn lcov_parser_rejects_impossible_function_supersets() {
    let config = detail_config(Some(1.0), None);
    for details in [
        ("FN:1,first\nFNDA:1,first\n", "FNF:2\nFNH:1\n"),
        (
            "FN:1,first\nFN:2,second\nFNDA:1,first\nFNDA:1,second\n",
            "FNF:1\nFNH:0\n",
        ),
    ] {
        assert_invalid_lcov(&config, &detail_report(details.0, details.1));
    }
}
