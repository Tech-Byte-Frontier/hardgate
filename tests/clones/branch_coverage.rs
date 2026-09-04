use super::clones::{clone_config, clone_pair};
use hardgate::engines::CloneDetector;
use std::path::{Path, PathBuf};

fn detector(min_lines: usize, min_tokens: usize) -> CloneDetector {
    let mut config = clone_config();
    config.min_lines = min_lines;
    config.min_tokens = min_tokens;
    CloneDetector::new(&config)
}

fn repeated_files(paths: &[&str], content: &str) -> Vec<(PathBuf, String)> {
    paths
        .iter()
        .map(|path| (PathBuf::from(path), content.to_owned()))
        .collect()
}

fn token_line(prefix: &str) -> String {
    format!("{prefix}_one {prefix}_two {prefix}_three\n")
}

fn separated_blocks(marker: &str) -> String {
    format!(
        "first_alpha first_beta first_gamma\n{marker}_one\n{marker}_two\n{marker}_three\n{marker}_four\nsecond_alpha second_beta second_gamma\n"
    )
}

fn shifted_block(prefix: &str, filler_lines: usize) -> String {
    let mut content = String::new();
    for index in 0..filler_lines {
        content.push_str(&format!("{prefix}_filler_{index}\n"));
    }
    content.push_str("stream_alpha stream_beta stream_gamma\n");
    content
}

#[test]
fn relative_paths_normalize_nested_roots_and_parents() {
    let detector = CloneDetector::new(&clone_config());
    for (root, first, second, expected_first, expected_second) in [
        (
            Path::new("workspace"),
            "workspace/a.rs",
            "workspace/b.rs",
            PathBuf::from("a.rs"),
            PathBuf::from("b.rs"),
        ),
        (
            Path::new("."),
            "../a.rs",
            "../b.rs",
            PathBuf::from("../a.rs"),
            PathBuf::from("../b.rs"),
        ),
    ] {
        let violations = detector
            .detect_clones_checked(&clone_pair(first, second), root)
            .unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].file_a, expected_first);
        assert_eq!(violations[0].file_b, expected_second);
    }
}

#[test]
fn invalid_exclude_globs_are_ignored() {
    let mut config = clone_config();
    config.excludes = Some(vec!["[".to_owned()]);
    let detector = CloneDetector::new(&config);
    let files = clone_pair("src/a.rs", "src/b.rs");

    assert!(detector.excluded_files(&files, Path::new(".")).is_empty());
}

#[test]
fn distinct_file_pairs_are_not_coalesced() {
    let detector = detector(1, 3);
    let content = token_line("pair");
    let files = repeated_files(&["a.rs", "b.rs", "c.rs"], &content);

    let violations = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap();
    assert_eq!(violations.len(), 3);
    assert_eq!(
        violations
            .iter()
            .map(|violation| (violation.file_a.clone(), violation.file_b.clone()))
            .collect::<Vec<_>>(),
        vec![
            (PathBuf::from("a.rs"), PathBuf::from("b.rs")),
            (PathBuf::from("a.rs"), PathBuf::from("c.rs")),
            (PathBuf::from("b.rs"), PathBuf::from("c.rs")),
        ]
    );
}

#[test]
fn distant_blocks_remain_separate_clone_spans() {
    let detector = detector(1, 3);
    let files = vec![
        (PathBuf::from("a.rs"), separated_blocks("left")),
        (PathBuf::from("b.rs"), separated_blocks("right")),
    ];

    let violations = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap();
    assert_eq!(violations.len(), 2);
    assert_eq!(violations[0].lines_a, (1, 1));
    assert_eq!(violations[1].lines_a, (6, 6));
}

#[test]
fn duplicate_paths_keep_stream_alignment_distinct() {
    let detector = detector(1, 3);
    let files = vec![
        (PathBuf::from("duplicate.rs"), shifted_block("zero", 0)),
        (PathBuf::from("duplicate.rs"), shifted_block("one", 8)),
        (PathBuf::from("duplicate.rs"), shifted_block("two", 16)),
    ];

    let violations = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap();
    assert_eq!(violations.len(), 2);
    assert_eq!(violations[0].lines_a, (1, 1));
    assert_eq!(violations[1].lines_a, (9, 9));
}

#[test]
fn offset_windows_reject_unaligned_token_ranges() {
    let detector = detector(1, 3);
    let block = "offset_alpha offset_beta offset_gamma";
    let left =
        format!("{block} filler_one filler_two filler_three filler_four filler_five {block}\n");
    let files = vec![
        (PathBuf::from("left.rs"), left),
        (PathBuf::from("right.rs"), format!("{block}\n")),
    ];

    let violations = detector
        .detect_clones_checked(&files, Path::new("."))
        .unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].lines_a, (1, 1));
    assert_eq!(violations[0].tokens, 3);
}
