#[path = "support/fs.rs"]
mod fs;
#[path = "support/reports.rs"]
mod reports;

use fs::tempdir;
use hardgate::GateReport;
use hardgate::commands::OutputOptions;
use hardgate::discovery::filter_files_by_paths;
use hardgate::engines::CloneViolation;
use reports::failing_report;
use std::path::{Path, PathBuf};

fn report_with_violations() -> GateReport {
    let mut report = failing_report();
    report.clone_violations.push(CloneViolation {
        file_a: PathBuf::from("src/main.rs"),
        lines_a: (1, 8),
        file_b: PathBuf::from("src/big.rs"),
        lines_b: (3, 10),
        tokens: 60,
        lines: 8,
        fingerprint: "fixture-fingerprint".to_string(),
        message: "clone".to_string(),
        recommendation: "Extract helper.".to_string(),
    });
    report.finalize(3, 9, 11);
    report
}

#[test]
fn test_compact_lists_each_violation_without_details() {
    let out = report_with_violations().render_compact();
    for needle in [
        "error[clone]",
        "error[complexity]",
        "error[file-budget]",
        "-->",
        "src/main.rs",
        "src/big.rs",
    ] {
        assert!(out.contains(needle), "missing {needle}");
    }
    assert!(!out.contains("help:"), "compact must skip help text");
    assert!(
        !out.contains("key contributors"),
        "compact must skip breakdowns"
    );
}

#[test]
fn test_summary_shows_totals_and_top_files() {
    let out = report_with_violations().render_summary();
    for needle in [
        "Summary: 3 errors",
        "1 clones",
        "Top files with violations:",
        "src/main.rs (2)",
        "result: fail (3 errors)",
    ] {
        assert!(out.contains(needle), "missing {needle}");
    }
    assert!(!out.contains("-->"), "summary must not list violations");
}

#[test]
fn test_json_embeds_summary_and_top_files() {
    let parsed: serde_json::Value =
        serde_json::from_str(&report_with_violations().render_json().unwrap()).unwrap();
    assert_eq!(parsed["summary"]["total_errors"], 3);
    assert_eq!(parsed["summary"]["clones"], 1);
    assert_eq!(parsed["summary"]["ast_violations"], 1);
    assert!(parsed["top_files"].as_array().unwrap().len() >= 2);
    assert_eq!(parsed["clone_violations"].as_array().unwrap().len(), 1);
    assert_eq!(
        parsed["clone_violations"][0]["fingerprint"],
        "fixture-fingerprint"
    );
}

#[test]
fn test_summary_json_is_lean() {
    let parsed: serde_json::Value =
        serde_json::from_str(&report_with_violations().render_summary_json().unwrap()).unwrap();
    assert_eq!(parsed["summary"]["total_errors"], 3);
    assert!(parsed["top_files"].is_array());
    assert!(
        parsed.get("clone_violations").is_none(),
        "summary JSON must omit full payloads"
    );
}

#[test]
fn test_output_options_flag_matrix() {
    let json = OutputOptions {
        json: true,
        ..Default::default()
    };
    assert!(json.is_json() && !json.is_compact() && !json.is_summary());

    let compact = OutputOptions {
        no_snippets: true,
        ..Default::default()
    };
    assert!(compact.is_compact() && !compact.is_json());

    let via_format = OutputOptions {
        format: Some("summary".to_string()),
        ..Default::default()
    };
    assert!(via_format.is_summary() && !via_format.is_json());
}

fn fixture_tree(tag: &str) -> (PathBuf, Vec<PathBuf>) {
    let root = tempdir(&format!("output-modes-{tag}"));
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let top = root.join("a.rs");
    let nested = sub.join("b.rs");
    std::fs::write(&top, "fn a() {}\n").unwrap();
    std::fs::write(&nested, "fn b() {}\n").unwrap();
    (root, vec![top, nested])
}

#[test]
fn test_filter_keeps_only_dir_matches() {
    let (root, files) = fixture_tree("dir");
    let kept = filter_files_by_paths(files, &[PathBuf::from("sub")], &root).unwrap();
    assert_eq!(kept, vec![root.join("sub").join("b.rs")]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_filter_accepts_single_file() {
    let (root, files) = fixture_tree("file");
    let target = root.join("a.rs");
    let kept = filter_files_by_paths(files, std::slice::from_ref(&target), Path::new(".")).unwrap();
    assert_eq!(kept, vec![target]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_filter_missing_path_errors() {
    let (root, files) = fixture_tree("missing");
    let err = filter_files_by_paths(files, &[PathBuf::from("nope.rs")], &root);
    assert!(err.is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_filter_empty_passes_through() {
    let (root, files) = fixture_tree("empty");
    let kept = filter_files_by_paths(files.clone(), &[], &root).unwrap();
    assert_eq!(kept, files);
    let _ = std::fs::remove_dir_all(&root);
}
