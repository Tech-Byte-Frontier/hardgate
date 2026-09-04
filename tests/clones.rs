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
        (PathBuf::from("src/a.rs"), "same\n".repeat(65)),
        (PathBuf::from("src/b.rs"), "same\n".repeat(65)),
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
        CloneIndexError::HashWindowCapacityExceeded { limit: 64, .. }
    ));
}

#[test]
fn checked_detector_reports_raw_match_truncation() {
    let detector = cap_test_detector();
    let content = one_token_lines(50_001, "token_");
    let files = vec![
        (PathBuf::from("src/a.rs"), content.clone()),
        (PathBuf::from("src/b.rs"), content),
    ];

    let error = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap_err();
    assert_eq!(
        error,
        CloneIndexError::RawMatchCapacityExceeded { limit: 50_000 }
    );
}

#[test]
fn static_snapshot_turns_raw_truncation_into_required_evidence() {
    let mut config = HardgateConfig::default();
    config.roles.fixture.clone_min_lines = Some(1);
    config.roles.fixture.clone_min_tokens = Some(1);
    let content = one_token_lines(50_001, "token_");
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
    assert!(finding.recommendation.contains("Raise clone thresholds"));
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
        (unchanged_path, "same\n".repeat(65)),
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
fn static_snapshot_turns_hash_truncation_into_required_evidence() {
    let mut config = HardgateConfig::default();
    config.roles.source.clone_min_lines = Some(1);
    config.roles.source.clone_min_tokens = Some(1);
    let repeated = format!(
        "fn repeated() {{\n{}\n}}\n",
        "    let same = 0;\n".repeat(65)
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
        finding.output.contains("Raise clone thresholds"),
        "{}",
        finding.output
    );
    assert!(
        finding.output.contains("do not add exclusions"),
        "{}",
        finding.output
    );
    assert!(finding.recommendation.contains("Raise clone thresholds"));
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
        "    let same = 0;\n".repeat(65)
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
            .contains("Raise clone thresholds")
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
