#[path = "clones/branch_coverage.rs"]
mod branch_coverage;
#[path = "support/clones.rs"]
mod clones;
#[path = "support/fs.rs"]
mod fixture_fs;

use clones::{clone_config, clone_pair};
use hardgate::commands::run_static_gate_snapshot;
use hardgate::config::HardgateConfig;
use hardgate::engines::{CloneDetector, clones::CloneIndexError};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn test_clone_detector() {
    let detector = CloneDetector::new(&clone_config());
    let files = clone_pair("src/a.rs", "src/b.rs");

    let violations = detector.detect_clones(&files, Path::new(".")).unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].file_a, PathBuf::from("src/a.rs"));
    assert_eq!(violations[0].file_b, PathBuf::from("src/b.rs"));
}

#[test]
fn test_clone_detector_excludes_advisory() {
    let mut config = clone_config();
    config.excludes = Some(vec!["src/excluded/**".to_string()]);

    let detector = CloneDetector::new(&config);
    let files = clone_pair("src/a.rs", "src/excluded/b.rs");

    assert_eq!(detector.count_excluded_files(&files, Path::new(".")), 1);

    let excluded = detector.excluded_files(&files, Path::new("."));
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0], &PathBuf::from("src/excluded/b.rs"));

    // The excluded file takes its clone out of scope.
    assert!(
        detector
            .detect_clones(&files, Path::new("."))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn test_clone_actual_tokens_not_threshold() {
    let detector = CloneDetector::new(&clone_config());
    let body = "let mut sum = 0;\n".repeat(12);
    let pair = vec![
        (
            PathBuf::from("src/a.rs"),
            format!("fn foo() {{\n{body}\n}}"),
        ),
        (
            PathBuf::from("src/b.rs"),
            format!("fn bar() {{\n{body}\n}}"),
        ),
    ];
    let violations = detector.detect_clones(&pair, Path::new(".")).unwrap();
    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].tokens >= 25,
        "tokens should be actual (>= min), got {}",
        violations[0].tokens
    );
}

#[test]
fn test_repeated_windows_are_bounded_and_deterministic() {
    let mut config = clone_config();
    config.min_lines = 1;
    config.min_tokens = 5;
    let detector = CloneDetector::new(&config);
    let repeated = "let value = source + 1;\n".repeat(2_000);
    let files = vec![
        (PathBuf::from("src/a.rs"), repeated.clone()),
        (PathBuf::from("src/b.rs"), repeated),
    ];
    let first = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap_err();
    let second = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap_err();
    assert_eq!(first, second);
    assert!(matches!(
        first,
        CloneIndexError::HashWindowCapacityExceeded { .. }
            | CloneIndexError::RawMatchCapacityExceeded { .. }
    ));
}

fn one_token_lines(count: usize, prefix: &str) -> String {
    (0..count)
        .map(|index| format!("{prefix}{index}\n"))
        .collect()
}

fn cap_test_detector() -> CloneDetector {
    let mut config = clone_config();
    config.min_lines = 1;
    config.min_tokens = 1;
    CloneDetector::new(&config)
}

#[test]
fn checked_detector_reports_hash_window_truncation_deterministically() {
    let detector = cap_test_detector();
    let files = vec![
        (PathBuf::from("src/a.rs"), "same\n".repeat(513)),
        (PathBuf::from("src/b.rs"), "same\n".repeat(513)),
    ];

    let first = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap_err();
    let second = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap_err();
    assert_eq!(first, second);
    assert!(matches!(
        first,
        CloneIndexError::HashWindowCapacityExceeded { limit: 512, .. }
    ));
}

#[test]
fn checked_detector_reports_raw_match_truncation() {
    let detector = cap_test_detector();
    let content = one_token_lines(200_001, "token_");
    let files = vec![
        (PathBuf::from("src/a.rs"), content.clone()),
        (PathBuf::from("src/b.rs"), content),
    ];

    let error = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap_err();
    assert_eq!(
        error,
        CloneIndexError::RawMatchCapacityExceeded { limit: 200_000 }
    );
}

#[test]
fn static_snapshot_turns_raw_truncation_into_required_evidence() {
    let mut config = HardgateConfig::default();
    config.roles.fixture.clone_min_lines = Some(1);
    config.roles.fixture.clone_min_tokens = Some(1);
    let content = one_token_lines(200_001, "token_");
    let files = vec![
        (PathBuf::from("tests/a.snap"), content.clone()),
        (PathBuf::from("tests/b.snap"), content),
    ];

    let report = run_static_gate_snapshot(&config, &files).unwrap().0;
    let finding = report
        .orchestration_violations
        .iter()
        .find(|finding| finding.step == "clone-index")
        .expect("raw truncation must be required evidence");
    assert!(finding.output.contains("raw clone-match capacity"));
    assert!(finding.output.contains("role Fixture"));
    assert!(finding.recommendation.contains("Retain the failing status"));
}

#[test]
fn checked_detector_reports_below_cap_clones() {
    let detector = cap_test_detector();
    let content = one_token_lines(1_000, "token_");
    let files = vec![
        (PathBuf::from("src/a.rs"), content.clone()),
        (PathBuf::from("src/b.rs"), content),
    ];

    let violations = detector
        .detect_clones_checked(&files, Path::new("."))
        .expect("below-cap clone index should be complete");
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].file_a, PathBuf::from("src/a.rs"));
    assert_eq!(violations[0].file_b, PathBuf::from("src/b.rs"));
}

#[test]
fn absolute_changed_paths_are_normalized_and_prioritized() {
    let detector = cap_test_detector();
    let root = Path::new(".");
    let absolute_root = std::env::current_dir().unwrap();
    let changed_path = absolute_root.join("src/z-changed.rs");
    let changed_path_with_dot = absolute_root.join("./src/z-changed.rs");
    let original_path = absolute_root.join("src/a-original.rs");
    let unchanged_path = absolute_root.join("src/m-unchanged.rs");
    let copied = one_token_lines(100, "token_");
    let clone_files = vec![
        (changed_path.clone(), copied.clone()),
        (original_path.clone(), copied.clone()),
    ];
    for root in [root, absolute_root.as_path()] {
        let violations = detector
            .detect_clones_checked_with_changed_files(
                &clone_files,
                root,
                std::slice::from_ref(&changed_path_with_dot),
            )
            .unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].file_a, PathBuf::from("src/a-original.rs"));
        assert_eq!(violations[0].file_b, PathBuf::from("src/z-changed.rs"));
    }
    let files = vec![
        (changed_path.clone(), copied.clone()),
        (original_path, copied),
        (unchanged_path, "same\n".repeat(513)),
    ];

    let error = detector
        .detect_clones_checked_with_changed_files(
            &files,
            root,
            std::slice::from_ref(&changed_path_with_dot),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        CloneIndexError::HashWindowCapacityExceeded { ref file, .. }
            if file == Path::new("src/m-unchanged.rs")
    ));
}

#[test]
fn absolute_paths_outside_root_remain_normalized_absolute() {
    let detector = cap_test_detector();
    let files = vec![
        (
            PathBuf::from("/tmp/hardgate-coverage-peer/../outside/a.rs"),
            one_token_lines(100, "token_"),
        ),
        (
            PathBuf::from("/tmp/hardgate-coverage-peer/../outside/b.rs"),
            one_token_lines(100, "token_"),
        ),
    ];

    let violations = detector
        .detect_clones_checked(&files, Path::new("/tmp/hardgate-coverage-root"))
        .unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].file_a, PathBuf::from("/tmp/outside/a.rs"));
    assert_eq!(violations[0].file_b, PathBuf::from("/tmp/outside/b.rs"));
}

#[test]
fn overlapping_clone_windows_coalesce_into_one_span() {
    let mut config = clone_config();
    config.min_lines = 1;
    config.min_tokens = 3;
    let detector = CloneDetector::new(&config);
    let content = one_token_lines(10, "unique_");
    let files = vec![
        (PathBuf::from("src/a.rs"), content.clone()),
        (PathBuf::from("src/b.rs"), content),
    ];

    let violations = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].lines_a, (1, 10));
    assert_eq!(violations[0].lines_b, (1, 10));
    assert_eq!(violations[0].lines, 10);
    assert_eq!(violations[0].tokens, 10);
}

#[test]
fn static_snapshot_turns_hash_truncation_into_required_evidence() {
    let mut config = HardgateConfig::default();
    config.roles.source.clone_min_lines = Some(1);
    config.roles.source.clone_min_tokens = Some(1);
    let repeated = format!(
        "fn repeated() {{\n{}\n}}\n",
        "    let same = 0;\n".repeat(513)
    );
    let files = vec![
        (PathBuf::from("src/a.rs"), repeated.clone()),
        (PathBuf::from("src/b.rs"), repeated),
    ];

    let first = run_static_gate_snapshot(&config, &files).unwrap().0;
    let second = run_static_gate_snapshot(&config, &files).unwrap().0;
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    let finding = first
        .orchestration_violations
        .iter()
        .find(|finding| finding.step == "clone-index")
        .expect("hash truncation must be required evidence");
    assert!(finding.output.contains("role Source"), "{}", finding.output);
    assert!(
        finding.output.contains("Retain the failing status"),
        "{}",
        finding.output
    );
    assert!(
        finding
            .output
            .contains("do not omit source or weaken policy"),
        "{}",
        finding.output
    );
    assert!(finding.recommendation.contains("Retain the failing status"));
}

fn write_fixture(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(target, content).unwrap();
}

fn fixture_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn run_fixture_hardgate(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .args(["check", "--diff", "--format", "json"])
        .current_dir(root)
        .output()
        .unwrap()
}

#[test]
fn diff_prioritizes_changed_files_but_blocks_on_late_hash_truncation() {
    let root = fixture_fs::tempdir("clone-cap-diff");
    write_fixture(
        &root,
        "hardgate.toml",
        r#"
[gate]
name = "clone-cap"
preset = "custom"
strict = true

[clones]
enabled = true
min_lines = 1
min_tokens = 1
"#,
    );
    let unchanged = format!(
        "fn repeated() {{\n{}\n}}\n",
        "    let same = 0;\n".repeat(513)
    );
    let copied = "fn copied() {\n    let total = 0;\n    total\n}\n";
    write_fixture(&root, "src/a-unchanged.rs", &unchanged);
    write_fixture(&root, "src/original.rs", copied);
    fixture_git(&root, &["init", "-q"]);
    fixture_git(&root, &["config", "user.email", "hardgate@example.invalid"]);
    fixture_git(&root, &["config", "user.name", "Hardgate Test"]);
    fixture_git(&root, &["config", "commit.gpgsign", "false"]);
    fixture_git(&root, &["add", "-A"]);
    fixture_git(&root, &["commit", "-qm", "baseline"]);
    write_fixture(&root, "src/z-changed.rs", copied);

    let first = run_fixture_hardgate(&root);
    let second = run_fixture_hardgate(&root);
    assert!(!first.status.success());
    let mut report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let mut second_report: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    report["duration_ms"] = serde_json::Value::Null;
    second_report["duration_ms"] = serde_json::Value::Null;
    assert_eq!(report, second_report);
    let finding = report["orchestration_violations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["step"] == "clone-index")
        .expect("diff cap exhaustion must block the gate");
    assert!(finding["output"].as_str().unwrap().contains("role Source"));
    assert!(
        finding["output"]
            .as_str()
            .unwrap()
            .contains("Retain the failing status")
    );
    let _ = std::fs::remove_dir_all(root);
}

fn first_fingerprint(files: &[(PathBuf, String)]) -> String {
    let detector = CloneDetector::new(&clone_config());
    let violations = detector.detect_clones(files, Path::new(".")).unwrap();
    assert_eq!(
        violations.len(),
        1,
        "expected one clone, got {violations:?}"
    );
    violations[0].fingerprint.clone()
}

#[test]
fn test_fingerprint_ignores_line_movement() {
    let baseline = clone_pair("src/a.rs", "src/b.rs");
    let moved = vec![
        (
            PathBuf::from("src/a.rs"),
            format!("fn prelude() {{ let noise = 99; }}\n\n{}", baseline[0].1),
        ),
        (
            PathBuf::from("src/b.rs"),
            format!("fn setup() {{ let other = 7; }}\n\n\n{}", baseline[1].1),
        ),
    ];

    assert_eq!(first_fingerprint(&baseline), first_fingerprint(&moved));
}

#[test]
fn test_fingerprint_survives_file_rename() {
    let baseline = clone_pair("src/a.rs", "src/b.rs");
    let renamed = vec![
        (PathBuf::from("renamed/a.rs"), baseline[0].1.clone()),
        (PathBuf::from("renamed/b.rs"), baseline[1].1.clone()),
    ];

    assert_eq!(first_fingerprint(&baseline), first_fingerprint(&renamed));
}

#[test]
fn test_fingerprint_is_independent_of_input_order() {
    let baseline = clone_pair("src/a.rs", "src/b.rs");
    let reversed = vec![baseline[1].clone(), baseline[0].clone()];

    assert_eq!(first_fingerprint(&baseline), first_fingerprint(&reversed));
}

#[test]
fn test_fingerprint_changes_with_normalized_token_content() {
    let baseline = clone_pair("src/a.rs", "src/b.rs");
    let changed = baseline
        .iter()
        .map(|(path, content)| (path.clone(), content.replace("i * 2", "i + 2")))
        .collect::<Vec<_>>();

    assert_ne!(first_fingerprint(&baseline), first_fingerprint(&changed));
}

#[test]
fn test_fingerprint_is_serialized_and_legacy_payloads_default() {
    let files = clone_pair("src/a.rs", "src/b.rs");
    let detector = CloneDetector::new(&clone_config());
    let violation = detector.detect_clones(&files, Path::new(".")).unwrap()[0].clone();
    let encoded = serde_json::to_value(&violation).unwrap();
    assert_eq!(encoded["fingerprint"], violation.fingerprint);

    let decoded: hardgate::engines::CloneViolation =
        serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded.fingerprint, violation.fingerprint);

    let mut legacy = encoded.as_object().unwrap().clone();
    legacy.remove("fingerprint");
    let decoded: hardgate::engines::CloneViolation =
        serde_json::from_value(serde_json::Value::Object(legacy)).unwrap();
    assert!(decoded.fingerprint.is_empty());
}

#[test]
fn routine_declarations_such_as_imports_and_type_aliases_are_ignored() {
    let detector = CloneDetector::new(&clone_config());

    // Matching Python import blocks must not be flagged as clones
    let py_a = r#"
import os
import sys
from typing import Dict, List, Optional, Tuple
from datetime import datetime, timezone

def calculate_area(radius):
    pi = 3.14159
    return pi * radius * radius
"#;
    let py_b = r#"
import os
import sys
from typing import Dict, List, Optional, Tuple
from datetime import datetime, timezone

def calculate_perimeter(length, width):
    return 2 * (length + width)
"#;
    let files = vec![
        (PathBuf::from("src/a.py"), py_a.to_string()),
        (PathBuf::from("src/b.py"), py_b.to_string()),
    ];
    let violations = detector.detect_clones(&files, Path::new(".")).unwrap();
    assert!(violations.is_empty(), "Python imports should not produce clone violations: {violations:?}");

    // Matching TypeScript import blocks and type aliases must not be flagged as clones
    let ts_a = r#"
import { useState, useEffect, useCallback } from 'react';
import type { FC, ReactNode } from 'react';
export type UserId = string;
export type UserProps = {
    id: UserId;
    name: string;
};

export function render_a(name: string) {
    const greeting = "hello " + name;
    return greeting.toUpperCase();
}
"#;
    let ts_b = r#"
import { useState, useEffect, useCallback } from 'react';
import type { FC, ReactNode } from 'react';
export type UserId = string;
export type UserProps = {
    id: UserId;
    name: string;
};

export function render_b(items: number[]) {
    let sum = 0;
    for (const item of items) {
        sum += item;
    }
    return sum;
}
"#;
    let files = vec![
        (PathBuf::from("src/a.tsx"), ts_a.to_string()),
        (PathBuf::from("src/b.tsx"), ts_b.to_string()),
    ];
    let violations = detector.detect_clones(&files, Path::new(".")).unwrap();
    assert!(violations.is_empty(), "TypeScript imports and type aliases should not produce clone violations: {violations:?}");
}

#[test]
fn schema_literal_repetitions_complete_without_capacity_error() {
    let detector = CloneDetector::new(&clone_config());
    // Repetitive schema fields up to 200 repetitions should complete without HashWindowCapacityExceeded
    let schema_content = (0..200)
        .map(|i| format!("field_{i}: string;\n"))
        .collect::<String>();
    let file_a = format!("export interface SchemaA {{\n{schema_content}}}\n");
    let file_b = format!("export interface SchemaB {{\n{schema_content}}}\n");
    let files = vec![
        (PathBuf::from("src/schema_a.ts"), file_a),
        (PathBuf::from("src/schema_b.ts"), file_b),
    ];
    let result = detector.detect_clones_checked(&files, Path::new("."));
    assert!(result.is_ok(), "200-repetition schema must not exceed capacity under limit 512: {:?}", result.err());
}
