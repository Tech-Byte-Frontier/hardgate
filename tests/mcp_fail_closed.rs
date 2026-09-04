#[path = "support/fs.rs"]
mod fs;

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn call_tool(root: &Path, name: &str, arguments: Value) -> Value {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .arg("mcp")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("MCP server should start");
    writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
    let output = child.wait_with_output().expect("MCP server should exit");
    assert!(output.status.success(), "MCP server failed: {output:?}");
    serde_json::from_slice(&output.stdout).expect("MCP response should be JSON")
}

fn tool_error_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("tool response should include text")
}

fn assert_tool_error(root: &Path, arguments: Value, expected: &str) {
    let response = call_tool(root, "hardgate_check", arguments);
    assert_eq!(response["result"]["isError"], true);
    assert!(tool_error_text(&response).contains(expected));
}

#[test]
fn scoped_missing_file_is_an_explicit_tool_error() {
    let root = fs::tempdir("mcp-missing");
    assert_tool_error(&root, json!({ "paths": ["missing.rs"] }), "Files not found");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unreadable_scoped_path_is_not_reported_as_a_pass() {
    let root = fs::tempdir("mcp-unreadable");
    std::fs::create_dir(root.join("not-a-file")).unwrap();
    assert_tool_error(
        &root,
        json!({ "paths": ["not-a-file"] }),
        "Unable to read required",
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn diff_discovery_requires_git_and_surfaces_failure() {
    let root = fs::tempdir("mcp-git-required");
    assert_tool_error(
        &root,
        json!({ "diff": true }),
        "Failed to discover source files",
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn malformed_tool_arguments_are_rejected() {
    let root = fs::tempdir("mcp-malformed");
    assert_tool_error(&root, json!({ "paths": [1] }), "only strings");
    let _ = std::fs::remove_dir_all(root);
}
