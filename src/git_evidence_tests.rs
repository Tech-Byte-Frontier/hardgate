use super::*;

fn assert_error<T>(result: Result<T>, message: &str) {
    let error = match result {
        Ok(_) => panic!("expected parser failure"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains(message),
        "expected {message} in error {error}"
    );
}

fn status_rejects(output: &[u8]) -> bool {
    parse_status(output).is_err()
}

fn diff_status_rejects(output: &[u8]) -> bool {
    parse_diff_status(output).is_err()
}

fn assert_parser_rejects(cases: &[(&str, fn(&[u8]) -> bool, &[u8])]) {
    for (label, parser, output) in cases {
        assert!(parser(output), "accepted {label} {output:?}");
    }
}

#[test]
fn split_nul_records_requires_termination_and_ignores_empty_records() {
    assert!(split_nul_records(b"", "Git status").unwrap().is_empty());
    assert_eq!(
        split_nul_records(b"first\0\0second\0", "Git status").unwrap(),
        vec![b"first".as_slice(), b"second".as_slice()]
    );
    assert_error(
        split_nul_records(b"unterminated", "Git status"),
        "not NUL terminated",
    );
}

#[test]
fn parse_status_and_diff_status_reject_hostile_records() {
    assert_parser_rejects(&[
        ("status", status_rejects, b"??".as_slice()),
        ("status", status_rejects, b" Msrc/lib.rs\0".as_slice()),
        ("status", status_rejects, b" M \0".as_slice()),
        ("status", status_rejects, b" M \xff.rs\0".as_slice()),
        ("status", status_rejects, b"R  src/new.rs\0".as_slice()),
        (
            "status",
            status_rejects,
            b"C  src/copy.rs\0../src/original.rs\0".as_slice(),
        ),
        (
            "diff status",
            diff_status_rejects,
            b"Z\0src/lib.rs\0".as_slice(),
        ),
        (
            "diff status",
            diff_status_rejects,
            b"R12\0src/old.rs\0src/new.rs\0".as_slice(),
        ),
        (
            "diff status",
            diff_status_rejects,
            b"R100\0src/old.rs\0".as_slice(),
        ),
        (
            "diff status",
            diff_status_rejects,
            b"M\0\xff.rs\0".as_slice(),
        ),
        (
            "diff status",
            diff_status_rejects,
            b"M\0/absolute.rs\0".as_slice(),
        ),
        (
            "diff status",
            diff_status_rejects,
            b"M\0../src/lib.rs\0".as_slice(),
        ),
        (
            "diff status",
            diff_status_rejects,
            b"M\0src/lib.rs".as_slice(),
        ),
    ]);
    let parsed = parse_status(b"?? src/new.rs\0 M README.md\0").unwrap();
    assert!(parsed.inventory_paths.contains(Path::new("src/new.rs")));
    assert!(parsed.untracked.contains(Path::new("src/new.rs")));
    assert!(!parsed.inventory_paths.contains(Path::new("README.md")));

    let parsed = parse_diff_status(b"R100\0src/old.rs\0src/new.rs\0M\0README.md\0").unwrap();
    assert!(parsed.paths.contains(Path::new("src/old.rs")));
    assert!(parsed.paths.contains(Path::new("src/new.rs")));
    assert!(parsed.rename_lineage.contains_key(Path::new("src/new.rs")));
    assert!(!parsed.paths.contains(Path::new("README.md")));
}

#[test]
fn normalize_path_rejects_absolute_parent_and_empty_paths() {
    for raw in [
        "",
        ".",
        "..",
        "../src/lib.rs",
        "/tmp/lib.rs",
        "src/../../lib.rs",
    ] {
        assert!(normalize_path(raw).is_err(), "accepted path {raw:?}");
    }
    assert_eq!(
        normalize_path("./src/./lib.rs").unwrap(),
        PathBuf::from("src/lib.rs")
    );
}

#[test]
fn parse_diff_path_handles_null_quoted_and_escaped_paths() {
    assert_eq!(parse_diff_path("/dev/null", "a/").unwrap(), None);
    assert_eq!(
        parse_diff_path("a/src/lib.rs\t", "a/").unwrap(),
        Some(PathBuf::from("src/lib.rs"))
    );
    assert_eq!(
        parse_diff_path(r#""a/src/old\tname.rs""#, "a/").unwrap(),
        Some(PathBuf::from("src/old\tname.rs"))
    );
    assert_eq!(
        parse_diff_path(r#""a/src/\101.rs""#, "a/").unwrap(),
        Some(PathBuf::from("src/A.rs"))
    );
}

#[test]
fn parse_diff_path_rejects_prefix_trailing_and_escape_errors() {
    for (value, prefix, message) in [
        ("src/lib.rs", "a/", "lacked prefix"),
        (r#""a/src/lib.rs"x"#, "a/", "malformed quoted diff path"),
        (r#""a/src/lib.rs"#, "a/", "unterminated quoted path"),
        (r#""a/src/\q.rs""#, "a/", "unknown quoted path escape"),
        ("\"a/src/lib.rs\\", "a/", "incomplete quoted path escape"),
        (r#""a/src/\777.rs""#, "a/", "octal escape overflowed"),
    ] {
        assert_error(parse_diff_path(value, prefix), message);
    }
}

#[test]
fn hunk_and_range_parsers_reject_malformed_or_unrepresentable_ranges() {
    for value in ["", "-", "1,x", "1,2,3", "usize::MAX"] {
        assert!(parse_range(value).is_none(), "accepted range {value:?}");
    }
    assert_eq!(parse_range("42"), Some((42, 1)));
    assert_eq!(parse_range("42,0"), Some((42, 0)));

    for header in ["", "@@", "@@ -x +1 @@", "@@ -1 +x @@", "@@ -1 +1 +2 @@"] {
        assert!(
            parse_hunk_header(header).is_err(),
            "accepted hunk {header:?}"
        );
    }
    assert_eq!(
        parse_hunk_header("@@ -2,3 +4,2 @@ context").unwrap(),
        (4, 2)
    );

    let mut lines = ChangedLineMap::new();
    add_hunk_lines(&mut lines, Path::new("src/lib.rs"), 8, 0).unwrap();
    assert!(lines.is_empty());
    assert_error(
        add_hunk_lines(&mut lines, Path::new("src/lib.rs"), 0, 1),
        "zero-based non-empty",
    );
    assert_error(
        add_hunk_lines(&mut lines, Path::new("src/lib.rs"), usize::MAX, 2),
        "range overflowed",
    );
}

#[test]
fn parse_diff_rejects_invalid_bytes_missing_paths_and_zero_ranges() {
    assert_error(parse_diff(b"\xff"), "not UTF-8");
    assert_error(parse_diff(b"@@ -1 +1 @@\n"), "no file path");
    assert_error(
        parse_diff(
            b"diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +0,1 @@\n",
        ),
        "zero-based non-empty",
    );

    let parsed = parse_diff(
        b"diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +2 @@\n+changed\n",
    )
    .unwrap();
    assert_eq!(parsed[Path::new("src/lib.rs")], BTreeSet::from([2]));
}

#[test]
fn diff_status_and_reference_hash_validators_are_strict() {
    for status in [
        b"".as_slice(),
        b"Z".as_slice(),
        b"M ".as_slice(),
        b"R12".as_slice(),
        b"R1x3".as_slice(),
    ] {
        assert!(!valid_diff_status(status), "accepted status {status:?}");
    }
    for status in [
        b"A".as_slice(),
        b"M".as_slice(),
        b"R100".as_slice(),
        b"C075".as_slice(),
    ] {
        assert!(valid_diff_status(status), "rejected status {status:?}");
    }

    assert!(!is_hex_hash(""));
    assert!(!is_hex_hash("0"));
    assert!(is_hex_hash(&"a".repeat(40)));
    assert!(is_hex_hash(&"A".repeat(64)));
    assert!(!is_hex_hash(&"a".repeat(39)));
    assert!(!is_hex_hash(&"g".repeat(40)));
}
