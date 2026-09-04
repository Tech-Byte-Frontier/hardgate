use hardgate::engines::util::{
    is_inside_string, is_offset_inside_string, strip_line_comment, strip_slash_comment,
};

#[test]
fn quote_helpers_handle_escapes_and_utf8_boundaries() {
    assert!(!is_inside_string(""));
    assert!(is_inside_string(r#"prefix "unterminated"#));
    assert!(!is_inside_string(r#"prefix "closed""#));
    assert!(!is_inside_string(r#"prefix 'escaped \''"#));

    let line = r#"let text = "café";"#;
    let opening_quote = line.find('"').expect("opening quote") + 1;
    assert!(is_offset_inside_string(line, opening_quote));

    // The offset points at the second byte of `é`; the helper must back up to
    // a UTF-8 boundary before slicing the prefix.
    let continuation_byte = line.find('é').expect("accented character") + 1;
    assert!(is_offset_inside_string(line, continuation_byte));
    assert!(!is_offset_inside_string(line, line.len() + 1));
}

#[test]
fn slash_comments_ignore_strings_escapes_and_backticks() {
    assert_eq!(
        strip_slash_comment(r#"const url = "https://example.test"; // trailing"#),
        r#"const url = "https://example.test"; "#
    );
    assert_eq!(
        strip_slash_comment(r#"const value = "quoted \" // inside"; // trailing"#),
        r#"const value = "quoted \" // inside"; "#
    );
    assert_eq!(
        strip_slash_comment("const value = `// inside`; // trailing"),
        "const value = `// inside`; "
    );
    assert_eq!(strip_slash_comment("value /"), "value /");
}

#[test]
fn hash_comments_ignore_strings_and_preserve_rust_attributes() {
    assert_eq!(
        strip_line_comment(r##"value = "# not a comment" # trailing"##),
        r##"value = "# not a comment" "##
    );
    assert_eq!(
        strip_line_comment("value = `# not a comment` # trailing"),
        "value = `# not a comment` "
    );
    assert_eq!(strip_line_comment("# shell comment"), "");
    assert_eq!(strip_line_comment("value // trailing"), "value ");

    // Rust attributes are code, not hash comments. These assertions document
    // the public helper contract and currently expose the implementation's
    // line-start/indentation handling gap.
    for source in [
        "#[derive(Debug)] struct Example;",
        "  #[inline] fn example() {}",
        "#![allow(dead_code)]",
        "  #![no_std]",
    ] {
        assert_eq!(strip_line_comment(source), source, "{source}");
    }
}
