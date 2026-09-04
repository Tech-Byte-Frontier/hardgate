#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, assert_status, json, run};
use hardgate::commands::cmd_scan_with_format;
use std::fs;

#[test]
fn scan_directory_read_failure_is_a_structured_json_error() {
    let fixture = Fixture::new("scan-functions", "directory-read", None);
    fs::create_dir(fixture.0.join("src")).unwrap();

    let output = run(fixture.as_ref(), &["scan", "src", "--format", "json"]);

    assert_status(&output, false, "scan directory");
    let failure = json(&output);
    assert_eq!(failure["passed"], false);
    assert_eq!(failure["stage"], "scan");
    assert_eq!(failure["kind"], "command-error");
    assert!(
        failure["message"]
            .as_str()
            .unwrap()
            .contains("Failed to read file")
    );

    fixture.write("clean.rs", "fn answer() -> i32 { 42 }\n");
    let output = run(fixture.as_ref(), &["scan", "clean.rs", "--json"]);
    assert_status(&output, true, "scan --json");
    assert_eq!(json(&output)["passed"], true);
}

#[test]
fn legacy_scan_helper_maps_format_and_preserves_read_context() {
    let root = std::env::temp_dir().join(format!("hardgate-scan-helper-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let source = root.join("clean.rs");
    fs::write(&source, "fn answer() -> i32 { 42 }\n").unwrap();

    cmd_scan_with_format(&source, Some("summary")).unwrap();
    let error = cmd_scan_with_format(&root, None).expect_err("directories are not source files");
    assert!(format!("{error:#}").contains("Failed to read file"));

    fs::remove_dir_all(root).unwrap();
}
