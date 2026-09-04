#[path = "support/fs.rs"]
mod fs;
#[path = "common/fs_git.rs"]
mod fs_git;

use fs_git::{commit_baseline, init_repo, write};
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const ROLE_POLICY_CONFIG: &str = r#"
[gate]
name = "mcp-role-policy"
preset = "custom"
strict = true

[budgets.files]
max_bytes = 100000

[budgets.files.max_lines]
default = 10000
rs = 10000

[budgets.functions]
max_cyclomatic = 100
max_cognitive = 100
max_parameters = 20
max_lines = 1000
max_nesting_depth = 20

[anti_gaming]
disallow_suppressions = true

[clones]
enabled = false

[classification]
[[classification.rules]]
glob = "src/**"
role = "test"

[roles.test]
severity = "warning"
max_cyclomatic = 1
"#;

const CLONE_CONFIG: &str = r#"
[gate]
preset = "custom"
strict = true

[budgets.files]
max_bytes = 100000

[budgets.files.max_lines]
default = 10000
rs = 10000

[budgets.functions]
max_cyclomatic = 100
max_cognitive = 100
max_parameters = 20
max_lines = 1000
max_nesting_depth = 20

[clones]
enabled = true
min_lines = 3
min_tokens = 10
"#;

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

fn assert_failed_source_scan(tag: &str, content: &[u8], expected_step: &str) {
    let root = fs::tempdir(tag);
    let path = root.join("src/broken.rs");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, content).unwrap();

    let response = call_tool(
        &root,
        "hardgate_check",
        json!({ "paths": ["src/broken.rs"] }),
    );
    assert_ne!(response["result"]["isError"], true);
    let text = tool_error_text(&response);
    assert!(text.contains("Hardgate Failed"), "{text}");
    assert!(text.contains(expected_step), "{text}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn scoped_missing_file_is_an_explicit_tool_error() {
    let root = fs::tempdir("mcp-missing");
    assert_tool_error(&root, json!({ "paths": ["missing.rs"] }), "Files not found");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn empty_scoped_directory_is_not_reported_as_a_pass() {
    let root = fs::tempdir("mcp-unreadable");
    std::fs::create_dir(root.join("not-a-file")).unwrap();
    assert_tool_error(
        &root,
        json!({ "paths": ["not-a-file"] }),
        "No source files matched",
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn read_failure_is_reported_as_a_failed_gate() {
    assert_failed_source_scan("mcp-read-failure", &[0xff], "read-source");
}

#[test]
fn parser_failure_is_reported_as_a_failed_gate() {
    assert_failed_source_scan("mcp-parser-failure", b"fn broken( {\n", "parse-source");
}

#[test]
fn invalid_config_is_an_explicit_tool_error() {
    let root = fs::tempdir("mcp-config-failure");
    write(&root, "hardgate.toml", "[gate\n");
    write(&root, "src/lib.rs", "fn okay() {}\n");
    assert_tool_error(
        &root,
        json!({ "paths": ["src/lib.rs"] }),
        "Failed to load hardgate.toml",
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

#[test]
fn empty_scope_is_an_explicit_tool_error() {
    let root = fs::tempdir("mcp-empty");
    assert_tool_error(
        &root,
        json!({ "paths": [] }),
        "No paths provided; refusing an empty successful check",
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn scoped_directory_uses_custom_role_policy() {
    let root = fs::tempdir("mcp-role-policy");
    write(&root, "hardgate.toml", ROLE_POLICY_CONFIG);
    write(
        &root,
        "src/policy.rs",
        "fn branch(value: bool) -> bool { if value { true } else { false } }\n",
    );

    let response = call_tool(&root, "hardgate_check", json!({ "paths": ["src"] }));
    assert_ne!(response["result"]["isError"], true);
    let text = tool_error_text(&response);
    assert!(text.contains("Hardgate Passed"), "{text}");
    assert!(text.contains("role Test advisory"), "{text}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn diff_scope_uses_full_clone_index() {
    let root = fs::tempdir("mcp-diff-clone");
    let copied = r#"
fn calculate_total(values: &[i32]) -> i32 {
    let mut total = 0;
    for value in values {
        if *value > 0 {
            total += *value;
        }
    }
    total
}
"#;
    write(&root, "hardgate.toml", CLONE_CONFIG);
    write(&root, "src/original.rs", copied);
    init_repo(&root);
    commit_baseline(&root, "baseline");
    write(&root, "src/copied.rs", copied);

    let response = call_tool(&root, "hardgate_check", json!({ "diff": true }));
    assert_ne!(response["result"]["isError"], true);
    let text = tool_error_text(&response);
    assert!(text.contains("Hardgate Failed"), "{text}");
    assert!(text.contains("src/copied.rs"), "{text}");
    assert!(text.contains("src/original.rs"), "{text}");

    let _ = std::fs::remove_dir_all(root);
}
