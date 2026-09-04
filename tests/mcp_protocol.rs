#[path = "support/fs.rs"]
mod fs;

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn run_mcp(root: &Path, input: &[u8]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .arg("mcp")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("MCP server should start");

    child
        .stdin
        .take()
        .expect("MCP server should expose stdin")
        .write_all(input)
        .expect("MCP request should be written");

    let output = child.wait_with_output().expect("MCP server should exit");
    assert!(
        output.status.success(),
        "MCP server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout)
        .expect("MCP responses should be UTF-8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("MCP response should be JSON: {error}: {line}"))
        })
        .collect()
}

fn request(id: u64, method: &str, params: Option<Value>) -> Value {
    let mut value = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
    });
    if let Some(params) = params {
        value["params"] = params;
    }
    value
}

fn ndjson(requests: impl IntoIterator<Item = Value>) -> Vec<u8> {
    let mut input = Vec::new();
    for request in requests {
        serde_json::to_writer(&mut input, &request).expect("request should serialize");
        input.push(b'\n');
    }
    input
}

fn framed(request: &Value, duplicate_length_header: bool) -> Vec<u8> {
    let payload = serde_json::to_vec(request).expect("request should serialize");
    let mut input = format!("Content-Length: {}\r\n", payload.len()).into_bytes();
    if duplicate_length_header {
        input.extend_from_slice(format!("Content-Length: {}\r\n", payload.len()).as_bytes());
    }
    input.extend_from_slice(b"\r\n");
    input.extend_from_slice(&payload);
    input.push(b'\n');
    input
}

fn response_error_code(response: &Value) -> i64 {
    response["error"]["code"]
        .as_i64()
        .expect("response should contain a JSON-RPC error code")
}

fn tool_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("tool response should contain text content")
}

#[test]
fn initialize_tools_list_and_notifications_use_line_framing() {
    let root = fs::tempdir("mcp-protocol-list");
    let initialize = request(
        1,
        "initialize",
        Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "mcp-protocol-test", "version": "1" }
        })),
    );
    let list = request(2, "tools/list", None);
    let ping = request(3, "ping", None);
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    let mut input = b"\n\n".to_vec();
    input.extend(ndjson([initialize, list, ping, notification]));
    let responses = run_mcp(&root, &input);
    assert_eq!(
        responses.len(),
        3,
        "notifications do not produce a response"
    );

    let initialize_response = &responses[0];
    assert_eq!(initialize_response["jsonrpc"], "2.0");
    assert_eq!(initialize_response["id"], 1);
    assert_eq!(
        initialize_response["result"]["protocolVersion"],
        "2024-11-05"
    );
    assert_eq!(
        initialize_response["result"]["serverInfo"]["name"],
        "hardgate"
    );
    assert_eq!(
        initialize_response["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert!(initialize_response["result"]["capabilities"]["tools"].is_object());

    let tools = responses[1]["result"]["tools"]
        .as_array()
        .expect("tools/list should return a tools array");
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool should have a name"))
        .collect();
    assert_eq!(
        names,
        vec![
            "hardgate_check",
            "hardgate_scan_file",
            "hardgate_get_metrics"
        ]
    );
    assert_eq!(tools[1]["inputSchema"]["required"], json!(["path"]));
    assert_eq!(
        tools[2]["inputSchema"]["required"],
        json!(["path", "symbol"])
    );
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(responses[2]["result"], json!({}));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn content_length_framing_accepts_duplicate_headers_with_same_length() {
    let root = fs::tempdir("mcp-protocol-framing");
    let responses = run_mcp(&root, &framed(&request(9, "ping", None), true));
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["jsonrpc"], "2.0");
    assert_eq!(responses[0]["id"], 9);
    assert_eq!(responses[0]["result"], json!({}));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn parse_invalid_request_and_unknown_method_errors_are_json_rpc_errors() {
    let root = fs::tempdir("mcp-protocol-errors");
    let mut input = b"{not valid json}\n".to_vec();
    input.extend(ndjson([
        json!({ "jsonrpc": "1.0", "id": "legacy", "method": "ping" }),
        request(12, "missing/method", None),
    ]));

    let responses = run_mcp(&root, &input);
    assert_eq!(responses.len(), 3);

    assert_eq!(responses[0]["id"], Value::Null);
    assert_eq!(response_error_code(&responses[0]), -32700);
    assert!(
        responses[0]["error"]["message"]
            .as_str()
            .expect("parse error should contain a message")
            .starts_with("Parse error:")
    );

    assert_eq!(responses[1]["id"], "legacy");
    assert_eq!(response_error_code(&responses[1]), -32600);
    assert_eq!(
        responses[1]["error"]["message"],
        "Invalid Request: jsonrpc must be '2.0'"
    );

    assert_eq!(responses[2]["id"], 12);
    assert_eq!(response_error_code(&responses[2]), -32601);
    assert_eq!(
        responses[2]["error"]["message"],
        "Method not found: missing/method"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn malformed_tool_arguments_fail_closed_without_process_errors() {
    let root = fs::tempdir("mcp-protocol-arguments");
    let requests = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call" }),
        request(2, "tools/call", Some(json!({}))),
        request(
            3,
            "tools/call",
            Some(json!({ "name": "hardgate_scan_file", "arguments": [] })),
        ),
        request(
            4,
            "tools/call",
            Some(json!({ "name": "hardgate_scan_file", "arguments": { "path": 9 } })),
        ),
        request(
            5,
            "tools/call",
            Some(json!({ "name": "unknown_tool", "arguments": {} })),
        ),
    ];

    let responses = run_mcp(&root, &ndjson(requests));
    assert_eq!(responses.len(), 5);
    for response in &responses {
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["result"]["isError"], true);
    }
    assert!(tool_text(&responses[0]).contains("Missing tool call parameters"));
    assert!(tool_text(&responses[1]).contains("Missing 'name' parameter"));
    assert!(tool_text(&responses[2]).contains("arguments' must be an object"));
    assert!(tool_text(&responses[3]).contains("Missing 'path' parameter"));
    assert!(tool_text(&responses[4]).contains("Unknown tool: unknown_tool"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn scan_tool_reports_success_and_read_failures() {
    let root = fs::tempdir("mcp-protocol-scan");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/good.rs"), "fn greet() {}\n").unwrap();
    std::fs::create_dir(root.join("src/directory.rs")).unwrap();

    let responses = run_mcp(
        &root,
        &ndjson([
            request(
                1,
                "tools/call",
                Some(json!({
                    "name": "hardgate_scan_file",
                    "arguments": { "path": "src/good.rs" }
                })),
            ),
            request(
                2,
                "tools/call",
                Some(json!({
                    "name": "hardgate_scan_file",
                    "arguments": { "path": "src/directory.rs" }
                })),
            ),
        ]),
    );
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["result"]["isError"], Value::Null);
    assert!(tool_text(&responses[0]).contains("Hardgate Passed"));
    assert!(tool_text(&responses[0]).contains("1 files and 1 functions"));
    assert_eq!(responses[1]["result"]["isError"], true);
    assert!(tool_text(&responses[1]).contains("Unable to read required source file(s)"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn metrics_tool_reports_success_missing_file_and_missing_symbol() {
    let root = fs::tempdir("mcp-protocol-metrics");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/metrics.rs"),
        "fn add(left: i32, right: i32) -> i32 { left + right }\n",
    )
    .unwrap();

    let responses = run_mcp(
        &root,
        &ndjson([
            request(
                1,
                "tools/call",
                Some(json!({
                    "name": "hardgate_get_metrics",
                    "arguments": { "path": "src/metrics.rs", "symbol": "add" }
                })),
            ),
            request(
                2,
                "tools/call",
                Some(json!({
                    "name": "hardgate_get_metrics",
                    "arguments": { "path": "src/missing.rs", "symbol": "add" }
                })),
            ),
            request(
                3,
                "tools/call",
                Some(json!({
                    "name": "hardgate_get_metrics",
                    "arguments": { "path": "src/metrics.rs", "symbol": "missing" }
                })),
            ),
        ]),
    );
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["result"]["isError"], Value::Null);
    let metrics = tool_text(&responses[0]);
    assert!(metrics.contains("\"name\": \"add\""));
    assert!(metrics.contains("\"cyclomatic\""));
    assert_eq!(responses[1]["result"]["isError"], true);
    assert!(tool_text(&responses[1]).contains("Cannot open: src/missing.rs"));
    assert_eq!(responses[2]["result"]["isError"], true);
    assert!(tool_text(&responses[2]).contains("Symbol 'missing' not found"));

    let _ = std::fs::remove_dir_all(root);
}
