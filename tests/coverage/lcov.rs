use super::{assert_invalid_lcov, detail_report, fs, metrics, strict_scorer};
use hardgate::config::CoverageConfig;
use hardgate::engines::CoverageScorer;
use std::path::Path;

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
    let config = CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(1.0),
        min_function_percent: Some(1.0),
        min_branch_percent: Some(1.0),
        max_crap_score: None,
        critical_paths: None,
    };
    let body = detail_report(
        "FN:1,2,compute\nFNDA:1,compute\nBRDA:1,0,case,with,comma,1\n",
        "FNF:1\nFNH:1\nBRF:1\nBRH:1",
    );
    let tmp = fs::tempdir("lcov-details-valid");
    let report = tmp.join("report.info");
    std::fs::write(&report, body).unwrap();
    let parsed = CoverageScorer::new(&config).parse_lcov(&report).unwrap();
    assert_eq!(parsed.len(), 1);
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn lcov_parser_rejects_function_detail_mismatches_and_duplicates() {
    let config = CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(1.0),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: None,
        critical_paths: None,
    };
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
fn lcov_parser_rejects_branch_detail_mismatches_and_malformed_fields() {
    let config = CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(1.0),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: None,
        critical_paths: None,
    };
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
fn lcov_parser_rejects_unknown_records_and_invalid_metadata_placement() {
    let config = CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(1.0),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: None,
        critical_paths: None,
    };
    for body in [
        "JUNK:1\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "TN\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
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
    let config = CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(1.0),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: None,
        critical_paths: None,
    };
    for body in [
        "LF:1\n",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\n",
        "SF:src/lib.rs\nLF:0\nLH:0\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:0,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1,checksum,extra\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nLF:2\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:0\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:./src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        "SF:src/./lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:src/lib.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    ] {
        assert_invalid_lcov(&config, body);
    }
}

#[test]
fn lcov_parser_requires_paired_function_and_branch_counts_when_floors_enabled() {
    let config = CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(1.0),
        min_function_percent: Some(1.0),
        min_branch_percent: Some(1.0),
        max_crap_score: None,
        critical_paths: None,
    };
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
