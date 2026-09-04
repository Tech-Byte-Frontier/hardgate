use super::{is_code_bearing, retain_code_lines};
use std::collections::BTreeSet;

#[test]
fn excludes_comments_and_delimiters_but_keeps_code() {
    let source = "// comment\n}\nlet answer = 42;\n/* block\n * more\n */\nanswer += 1;\n";
    let lines = BTreeSet::from([1, 2, 3, 4, 5, 6]);
    assert_eq!(retain_code_lines(source, &lines), BTreeSet::from([3]));
}

#[test]
fn retains_zero_and_out_of_range_candidates() {
    let lines = BTreeSet::from([0, 1, 3]);
    assert_eq!(
        retain_code_lines("let answer = 1;\n", &lines),
        BTreeSet::from([0, 1, 3])
    );
}

#[test]
fn ignores_trailing_blank_line() {
    let lines = BTreeSet::from([2]);
    assert!(retain_code_lines("one\n", &lines).is_empty());
}

#[test]
fn advances_block_comment_state_on_unchanged_lines() {
    let source = "/* unchanged opener\nstill comment\n*/\nlet answer = 1;\n";
    let lines = BTreeSet::from([4]);
    assert_eq!(retain_code_lines(source, &lines), lines);
}

#[test]
fn resumes_after_unchanged_block_comment_closer() {
    let source = "/*\n*/\nlet answer = 1;\n";
    let lines = BTreeSet::from([3]);
    assert_eq!(retain_code_lines(source, &lines), lines);
}

#[test]
fn tracks_nested_block_comments_before_changed_code() {
    let source = "/* outer\n/* inner */\nstill outer\n*/\nlet answer = 1;\n";
    let lines = BTreeSet::from([3, 5]);
    assert_eq!(retain_code_lines(source, &lines), BTreeSet::from([5]));
}

#[test]
fn preserves_multiline_template_literal_comment_markers() {
    let source = "const text = `open\n// literal text\n`;\n";
    let lines = BTreeSet::from([2]);
    assert_eq!(retain_code_lines(source, &lines), lines);
}

#[test]
fn preserves_quoted_escapes_including_trailing_escape() {
    let source = r##"let quoted = "escaped \" // literal";
let open = "trailing\
let still_open = 1;
"##;
    let lines = BTreeSet::from([1, 2, 3]);
    assert_eq!(retain_code_lines(source, &lines), lines);
}

#[test]
fn preserves_multiline_raw_string_comment_markers() {
    let source = "let text = r#\"open\n// literal text\n\"#;\nlet plain = r\"literal\";\n";
    let lines = BTreeSet::from([2, 4]);
    assert_eq!(retain_code_lines(source, &lines), lines);
}

#[test]
fn handles_raw_hash_mismatches_before_a_matching_close() {
    let source = "let text = r###\"open\n// literal text\n\"##x\n\"###\nlet answer = 1;\n";
    let lines = BTreeSet::from([2, 3, 4, 5]);
    assert_eq!(retain_code_lines(source, &lines), lines);

    let unterminated = "let text = r###\"open\n\"\n";
    let lines = BTreeSet::from([2]);
    assert_eq!(retain_code_lines(unterminated, &lines), lines);
}

#[test]
fn keeps_hash_attributes_and_shebangs_but_drops_hash_comments() {
    let source = "#!/usr/bin/env rust\n#[derive(Debug)]\n# plain comment\nlet value = 1; # trailing comment\n";
    let lines = BTreeSet::from([1, 2, 3, 4]);
    assert_eq!(retain_code_lines(source, &lines), BTreeSet::from([1, 2, 4]));
}

#[test]
fn drops_markup_and_delimiter_only_lines() {
    let source = "<!-- comment -->\n-- comment\n{}\nlet value = 1;\n";
    let lines = BTreeSet::from([1, 2, 3, 4]);
    assert_eq!(retain_code_lines(source, &lines), BTreeSet::from([4]));

    let division = "let ratio = numerator / denominator;\n";
    assert_eq!(
        retain_code_lines(division, &BTreeSet::from([1])),
        BTreeSet::from([1])
    );
}

#[test]
fn recognizes_code_bearing_predicate_edges() {
    assert!(!is_code_bearing("   "));
    assert!(!is_code_bearing("// comment"));
    assert!(!is_code_bearing("# comment"));
    assert!(!is_code_bearing("<!-- comment -->"));
    assert!(!is_code_bearing("-- comment"));
    assert!(!is_code_bearing("{}[]();,:"));
    assert!(is_code_bearing("#[derive(Debug)]"));
    assert!(is_code_bearing("#![no_std]"));
    assert!(is_code_bearing("let value = 1;"));
}
