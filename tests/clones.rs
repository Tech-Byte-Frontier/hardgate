#[path = "support/clones.rs"]
mod clones;

use clones::{clone_config, clone_pair};
use hardgate::engines::CloneDetector;
use std::path::{Path, PathBuf};

#[test]
fn test_clone_detector() {
    let detector = CloneDetector::new(&clone_config());
    let files = clone_pair("src/a.rs", "src/b.rs");

    let violations = detector.detect_clones(&files, Path::new("."));
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
    assert!(detector.detect_clones(&files, Path::new(".")).is_empty());
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
    let violations = detector.detect_clones(&pair, Path::new("."));
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
    let first = detector.detect_clones(&files, Path::new("."));
    let second = detector.detect_clones(&files, Path::new("."));
    assert_eq!(first.len(), second.len());
    assert!(
        first.len() < 1_000,
        "bounded index emitted {} clones",
        first.len()
    );
}
