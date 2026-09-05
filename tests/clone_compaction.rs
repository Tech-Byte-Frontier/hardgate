use hardgate::config::CloneConfig;
use hardgate::engines::{CloneDetector, clones::CloneIndexError};
use std::path::{Path, PathBuf};

fn detector(min_lines: usize, min_tokens: usize) -> CloneDetector {
    CloneDetector::new(&CloneConfig {
        enabled: true,
        min_lines,
        min_tokens,
        excludes: None,
    })
}

fn fingerprint(files: Vec<(PathBuf, String)>) -> (String, usize) {
    let violations = detector(1, 3)
        .detect_clones_checked(&files, Path::new("."))
        .unwrap();
    let violation = violations
        .iter()
        .max_by_key(|violation| violation.tokens)
        .expect("normalized bodies should produce a clone");
    (violation.fingerprint.clone(), violation.tokens)
}

fn normalized_body(number: &str, string: &str, comment: &str) -> String {
    format!(
        "fn sample() {{\n    let café = {number};\n    let message = {string}; // {comment}\n    return café;\n}}\n"
    )
}

#[test]
fn interning_preserves_normalized_tokens_and_fingerprint_bytes() {
    let baseline = fingerprint(vec![
        (
            PathBuf::from("src/a.rs"),
            normalized_body("10", "\"first\"", "one"),
        ),
        (
            PathBuf::from("src/b.rs"),
            normalized_body("20", "'second'", "two"),
        ),
    ]);
    // Captured from the source-equivalent release binary with this fixture,
    // using [clones] min_lines = 1 and min_tokens = 3.
    assert_eq!(
        baseline,
        ("1850ca82e0bf9553".to_owned(), 21),
        "the compact index must retain the established fingerprint contract"
    );
    let changed_literals_and_comment = fingerprint(vec![
        (
            PathBuf::from("src/a.rs"),
            normalized_body("1000", "`different`", "alpha"),
        ),
        (
            PathBuf::from("src/b.rs"),
            normalized_body("2000", "\"another\"", "beta"),
        ),
    ]);

    assert_eq!(baseline, changed_literals_and_comment);
}

#[test]
fn duplicate_paths_keep_spaced_streams_and_same_line_suppression() {
    let duplicate_streams = vec![
        (PathBuf::from("duplicate.rs"), shifted_body("zero", 0)),
        (PathBuf::from("duplicate.rs"), shifted_body("one", 8)),
        (PathBuf::from("duplicate.rs"), shifted_body("two", 16)),
    ];
    let violations = detector(1, 3)
        .detect_clones_checked(&duplicate_streams, Path::new("."))
        .unwrap();
    assert_eq!(violations.len(), 2);
    assert_eq!(violations[0].lines_a, (1, 1));
    assert_eq!(violations[1].lines_a, (9, 9));

    let same_line = vec![
        (
            PathBuf::from("duplicate.rs"),
            "shared_alpha shared_beta shared_gamma\n".to_owned(),
        ),
        (
            PathBuf::from("duplicate.rs"),
            "shared_alpha shared_beta shared_gamma\n".to_owned(),
        ),
    ];
    assert!(
        detector(1, 3)
            .detect_clones_checked(&same_line, Path::new("."))
            .unwrap()
            .is_empty()
    );

    let repeated = "same\n".repeat(65);
    let copied = "copied_alpha copied_beta copied_gamma\n";
    let files = vec![
        (PathBuf::from("z-changed.rs"), copied.to_owned()),
        (PathBuf::from("a-original.rs"), copied.to_owned()),
        (PathBuf::from("m-unchanged.rs"), repeated),
    ];
    let error = detector(1, 1)
        .detect_clones_checked_with_changed_files(
            &files,
            Path::new("."),
            &[PathBuf::from("z-changed.rs")],
        )
        .unwrap_err();
    assert!(matches!(
        error,
        CloneIndexError::HashWindowCapacityExceeded { ref file, line: 65, limit: 64 }
            if file == Path::new("m-unchanged.rs")
    ));
}

fn shifted_body(prefix: &str, filler_lines: usize) -> String {
    let mut content = String::new();
    for index in 0..filler_lines {
        content.push_str(&format!("{prefix}_filler_{index}\n"));
    }
    content.push_str("stream_alpha stream_beta stream_gamma\n");
    content
}

fn long_body(lines: usize) -> String {
    (0..lines)
        .map(|index| format!("shared_alpha_{index} shared_beta_{index} shared_gamma_{index}\n"))
        .collect()
}

#[test]
fn aligned_and_shifted_windows_coalesce_without_rechecking_the_verified_prefix() {
    let body = long_body(140);
    let aligned = detector(1, 3)
        .detect_clones_checked(
            &[
                (PathBuf::from("src/a.rs"), body.clone()),
                (PathBuf::from("src/b.rs"), body.clone()),
            ],
            Path::new("."),
        )
        .unwrap();
    assert_eq!(aligned.len(), 1);
    assert_eq!(aligned[0].lines_a, (1, 140));
    assert_eq!(aligned[0].lines_b, (1, 140));
    assert_eq!(aligned[0].tokens, 140 * 3);

    let shifted = detector(1, 3)
        .detect_clones_checked(
            &[
                (
                    PathBuf::from("src/a.rs"),
                    format!("unique_prefix unique_value unique_suffix\n{body}"),
                ),
                (PathBuf::from("src/b.rs"), body),
            ],
            Path::new("."),
        )
        .unwrap();
    assert_eq!(shifted.len(), 1);
    assert_eq!(shifted[0].lines_a, (2, 141));
    assert_eq!(shifted[0].lines_b, (1, 140));
    assert_eq!(shifted[0].tokens, 140 * 3);
}

#[test]
fn repeated_cross_products_keep_the_explicit_capacity_error() {
    let repeated = "same\n".repeat(65);
    let error = detector(1, 3)
        .detect_clones_checked(
            &[
                (PathBuf::from("src/a.rs"), repeated.clone()),
                (PathBuf::from("src/b.rs"), repeated),
            ],
            Path::new("."),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        CloneIndexError::HashWindowCapacityExceeded { limit: 64, .. }
    ));
}

#[test]
fn opposite_direction_windows_keep_the_verified_clone() {
    let files = vec![
        (PathBuf::from("src/a.rs"), "a\na\na\nb\na\n".to_owned()),
        (PathBuf::from("src/b.rs"), "a\nb\na\na\na\n".to_owned()),
    ];
    let violations = detector(1, 3)
        .detect_clones_checked(&files, Path::new("."))
        .unwrap();

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].lines_a, (1, 3));
    assert_eq!(violations[0].lines_b, (3, 5));
    assert_eq!(violations[0].tokens, 3);
}

#[test]
fn input_permutations_have_identical_ordered_findings() {
    let body = long_body(12);
    let files = vec![
        (PathBuf::from("src/c.rs"), body.clone()),
        (PathBuf::from("src/a.rs"), body.clone()),
        (PathBuf::from("src/b.rs"), body),
    ];
    let reversed = files.iter().cloned().rev().collect::<Vec<_>>();
    let first = detector(1, 3)
        .detect_clones_checked(&files, Path::new("."))
        .unwrap();
    let second = detector(1, 3)
        .detect_clones_checked(&reversed, Path::new("."))
        .unwrap();
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
}
