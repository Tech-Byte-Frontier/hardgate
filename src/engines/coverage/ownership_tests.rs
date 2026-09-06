use super::*;
use crate::fs_tests;

#[test]
fn production_coverage_floors_cannot_be_inflated_by_inline_test_records() {
    let root = fs_tests::tempdir("coverage-rust-ownership");
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn production() -> i32 { 1 }\n#[cfg(test)]\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    let path = root.join("report.lcov");
    std::fs::write(&path, "SF:src/lib.rs\nFN:1,production\nFN:3,helper\nFNDA:0,production\nFNDA:1,helper\nFNF:2\nFNH:1\nDA:1,0\nDA:3,1\nLF:2\nLH:1\nBRDA:1,0,0,0\nBRDA:3,0,0,1\nBRF:2\nBRH:1\nend_of_record\n").unwrap();
    let mut config = HardgateConfig::default();
    config.coverage.min_line_percent = Some(40.0);
    config.coverage.min_function_percent = Some(40.0);
    config.coverage.min_branch_percent = Some(40.0);
    let scorer = CoverageScorer::new(&config.coverage);
    let map = scorer
        .parse_lcov_for_project(&path, &root, &config)
        .unwrap();
    let record = &map[Path::new("src/lib.rs")];
    assert_eq!(
        (
            record.lines_found,
            record.functions_found,
            record.branches_found
        ),
        (1, 1, 1)
    );
    assert_eq!(
        (record.lines_hit, record.functions_hit, record.branches_hit),
        (0, 0, 0)
    );
    assert_eq!(scorer.evaluate(&map, &[], &root).len(), 3);
    // Aggregate-only function counts cannot be safely assigned to source/test.
    std::fs::write(
        &path,
        "SF:src/lib.rs\nFNF:2\nFNH:1\nDA:1,0\nDA:3,1\nLF:2\nLH:1\nBRF:0\nBRH:0\nend_of_record\n",
    )
    .unwrap();
    let error = scorer
        .parse_lcov_for_project(&path, &root, &config)
        .unwrap_err();
    assert!(format!("{error:#}").contains("FN/FNDA"), "{error:#}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mixed_source_test_line_is_not_credited_as_production() {
    let root = fs_tests::tempdir("coverage-mixed-line");
    std::fs::write(
        root.join("lib.rs"),
        "pub fn production() {} #[cfg(test)] fn helper() {}\n",
    )
    .unwrap();
    let path = root.join("report.lcov");
    std::fs::write(
        &path,
        "SF:lib.rs\nDA:1,1\nLF:1\nLH:1\nFNF:2\nFNH:2\nBRF:0\nBRH:0\nend_of_record\n",
    )
    .unwrap();
    let config = HardgateConfig::default();
    let error = CoverageScorer::new(&config.coverage)
        .parse_lcov_for_project(&path, &root, &config)
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("mixes production and test"),
        "{error:#}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn overlapping_summary_lines_keep_unattributed_counts_without_test_hit_credit() {
    let root = fs_tests::tempdir("coverage-summary-overlap");
    std::fs::write(
        root.join("lib.rs"),
        "pub fn production() {}\n#[cfg(test)]\nfn helper() {}\n",
    )
    .unwrap();
    let path = root.join("report.lcov");
    let config = HardgateConfig::default();
    for (summary_hits, branch_hits, expected_hits) in [(3, 2, 1), (1, 1, 0)] {
        std::fs::write(&path, format!("SF:lib.rs\nFN:1,production\nFN:3,helper\nFNDA:1,production\nFNDA:1,helper\nFNF:2\nFNH:2\nDA:1,1\nDA:3,1\nLF:3\nLH:{summary_hits}\nBRDA:1,0,0,1\nBRDA:3,0,0,1\nBRF:2\nBRH:{branch_hits}\nend_of_record\n")).unwrap();
        let map = CoverageScorer::new(&config.coverage)
            .parse_lcov_for_project(&path, &root, &config)
            .unwrap();
        let record = &map[Path::new("lib.rs")];
        assert_eq!(record.lines_found, 2);
        assert_eq!(record.lines_hit, expected_hits);
        assert_eq!(record.branches_found, 1);
        assert_eq!(record.branches_hit, expected_hits);
        assert_eq!(record.line_hits.len(), 1);
        assert!(!record.line_hits.contains_key(&3));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn monomorphized_functions_require_consistent_grouped_totals_for_test_projection() {
    let root = fs_tests::tempdir("coverage-function-groups");
    std::fs::write(
        root.join("lib.rs"),
        "pub fn production<T>() {}\n#[cfg(test)]\nfn helper() {}\n",
    )
    .unwrap();
    let path = root.join("report.lcov");
    let config = HardgateConfig::default();
    for (found, hit, accepted) in [(2, 2, true), (2, 1, false), (3, 3, false)] {
        std::fs::write(&path, format!("SF:lib.rs\nFN:1,production_u8\nFN:1,production_u16\nFN:3,helper\nFNDA:1,production_u8\nFNDA:0,production_u16\nFNDA:1,helper\nFNF:{found}\nFNH:{hit}\nDA:1,1\nDA:3,1\nLF:2\nLH:2\nBRF:0\nBRH:0\nend_of_record\n")).unwrap();
        let result =
            CoverageScorer::new(&config.coverage).parse_lcov_for_project(&path, &root, &config);
        assert_eq!(result.is_ok(), accepted, "{found}/{hit}: {result:?}");
        if let Ok(map) = result {
            let record = &map[Path::new("lib.rs")];
            assert_eq!((record.functions_found, record.functions_hit), (1, 1));
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
