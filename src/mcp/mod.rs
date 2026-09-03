use crate::commands::check::{AnalyzeInput, analyze_file_content};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{DiscoverOptions, discover_files_with_exclusions};
use crate::engines::{AntiGamingScanner, CloneDetector, ComplexityAnalyzer, InvariantsChecker};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
}

/// Serve the Model Context Protocol over stdio: `hardgate_check`,
/// `hardgate_scan_file`, and `hardgate_get_metrics` tools for AI assistants.
pub fn run_mcp_server() -> Result<()> {
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut stdout = io::stdout();

    // Support both newline-delimited JSON-RPC (simple clients) and
    // LSP-style `Content-Length: N` framing (VS Code / strict clients).
    loop {
        let Some(message) = read_mcp_message(&mut stdin_lock)? else {
            break;
        };
        let trimmed = message.trim();
        if !trimmed.is_empty() {
            process_mcp_line(trimmed, &mut stdout)?;
        }
    }

    Ok(())
}

fn read_mcp_message<R: BufRead>(reader: &mut R) -> Result<Option<String>> {
    let mut first = String::new();
    let n = reader.read_line(&mut first)?;
    if n == 0 {
        return Ok(None);
    }
    // LSP framing: `Content-Length: <bytes>\r\n\r\n<json>`
    if let Some(len) = parse_content_length(&first) {
        // Consume remaining header lines until blank.
        loop {
            let mut hdr = String::new();
            reader.read_line(&mut hdr)?;
            if hdr.trim().is_empty() {
                break;
            }
            // Allow re-specification (last wins) — uncommon but harmless.
            if let Some(l) = parse_content_length(&hdr) {
                let _ = l;
            }
        }
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf)?;
        let s = String::from_utf8_lossy(&buf).to_string();
        return Ok(Some(s));
    }
    Ok(Some(first))
}

fn parse_content_length(line: &str) -> Option<usize> {
    let lower = line.to_ascii_lowercase();
    let rest = lower.strip_prefix("content-length:")?;
    rest.trim().parse::<usize>().ok()
}

fn process_mcp_line<W: Write>(line: &str, out: &mut W) -> Result<()> {
    let request = match serde_json::from_str::<JsonRpcRequest>(line) {
        Ok(req) => req,
        Err(e) => return send_parse_error(out, e),
    };

    if request.jsonrpc != "2.0" {
        return send_invalid_request_error(out, request.id);
    }

    dispatch_mcp_method(&request, out)
}

fn dispatch_mcp_method<W: Write>(req: &JsonRpcRequest, out: &mut W) -> Result<()> {
    let id = req.id.clone().unwrap_or(serde_json::Value::Null);

    if req.method == "initialize" {
        let res = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "hardgate", "version": env!("CARGO_PKG_VERSION") }
        });
        return send_success(out, id, res);
    }
    if req.method == "ping" {
        return send_success(out, id, json!({}));
    }
    if req.method == "tools/list" {
        return send_success(out, id, get_tools_list());
    }
    if req.method == "tools/call" {
        let res = handle_tool_call(req.params.as_ref());
        return send_success(out, id, res);
    }
    if req.method == "notifications/initialized" {
        return Ok(());
    }

    send_method_not_found(out, id, &req.method)
}

fn get_tools_list() -> serde_json::Value {
    json!({
        "tools": [
            {
                "name": "hardgate_check",
                "description": "Deterministic quality gate verification (budgets, anti-gaming, AST complexity, architectural boundaries).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional list of file paths to check."
                        }
                    }
                }
            },
            {
                "name": "hardgate_scan_file",
                "description": "Scans a single file for complexity metrics, suppressions, and budgets.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "Path to file" } },
                    "required": ["path"]
                }
            },
            {
                "name": "hardgate_get_metrics",
                "description": "Retrieves cyclomatic, cognitive, parameter, and line metrics for a function symbol.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "symbol": { "type": "string" }
                    },
                    "required": ["path", "symbol"]
                }
            }
        ]
    })
}

fn handle_tool_call(params: Option<&serde_json::Value>) -> serde_json::Value {
    let Some(params) = params else {
        return tool_error("Missing tool call parameters");
    };

    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    match name {
        "hardgate_check" => execute_check_tool(&args),
        "hardgate_scan_file" => execute_scan_tool(&args),
        "hardgate_get_metrics" => execute_metrics_tool(&args),
        _ => tool_error(&format!("Unknown tool: {}", name)),
    }
}

fn execute_check_tool(args: &serde_json::Value) -> serde_json::Value {
    let config = match HardgateConfig::load_or_default(None) {
        Ok(c) => c,
        Err(e) => return tool_error(&format!("Failed to load hardgate.toml: {}", e)),
    };
    let root = Path::new(".");
    if let Some(arr) = args.get("paths").and_then(|p| p.as_array()) {
        return execute_scoped_check(arr, &config, root);
    }

    let discovery = discover_files_with_exclusions(DiscoverOptions {
        root,
        diff_only: false,
        exclusions: &config.budgets.files.exclusions.paths,
    })
    .unwrap_or_default();
    finish_check_response(CheckResponseInput {
        target_files: &discovery.files,
        excluded_count: discovery.excluded_files.len(),
        config: &config,
        root,
        extra_advisories: Vec::new(),
    })
}

fn execute_scoped_check(
    arr: &[serde_json::Value],
    config: &HardgateConfig,
    root: &Path,
) -> serde_json::Value {
    let (files, missing) = partition_existing_paths(arr);
    if files.is_empty() && !missing.is_empty() {
        return tool_error(&format!("Files not found: {}", missing.join(", ")));
    }
    let mut skipped = Vec::new();
    if !missing.is_empty() {
        skipped.push(format!(
            "Skipped {} missing path(s): {}.",
            missing.len(),
            missing.join(", ")
        ));
    }
    finish_check_response(CheckResponseInput {
        target_files: &files,
        excluded_count: 0,
        config,
        root,
        extra_advisories: skipped,
    })
}

fn partition_existing_paths(arr: &[serde_json::Value]) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut missing = Vec::new();
    for p in arr.iter().filter_map(|p| p.as_str()) {
        let pb = PathBuf::from(p);
        if pb.exists() {
            files.push(pb);
        } else {
            missing.push(p.to_string());
        }
    }
    (files, missing)
}

struct CheckResponseInput<'a> {
    target_files: &'a [PathBuf],
    excluded_count: usize,
    config: &'a HardgateConfig,
    root: &'a Path,
    extra_advisories: Vec<String>,
}

fn finish_check_response(input: CheckResponseInput) -> serde_json::Value {
    let CheckResponseInput {
        target_files,
        excluded_count,
        config,
        root,
        mut extra_advisories,
    } = input;
    let mut report = GateReport::new(config.gate.name.clone());
    report.advisories.append(&mut extra_advisories);
    if excluded_count > 0 {
        let noun = if excluded_count == 1 { "file" } else { "files" };
        report.advisories.push(format!(
            "{} {} excluded from file budget checks via hardgate.toml.",
            excluded_count, noun
        ));
    }
    let read_results = read_files_content(target_files);
    let func_count = analyze_file_list(target_files, config, root, &mut report);
    append_clone_results(&read_results, config, root, &mut report);

    report.finalize(target_files.len(), func_count, 0);
    json!({ "content": [{ "type": "text", "text": report.render_agent() }] })
}

fn read_files_content(paths: &[PathBuf]) -> Vec<(PathBuf, String)> {
    paths
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok().map(|c| (p.clone(), c)))
        .collect()
}

fn append_clone_results(
    files: &[(PathBuf, String)],
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) {
    if !config.clones.enabled || files.len() < 2 {
        return;
    }
    let detector = CloneDetector::new(&config.clones);
    report
        .clone_violations
        .extend(detector.detect_clones(files, root));
}

fn get_str_arg<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, serde_json::Value> {
    args.get(key)
        .and_then(|p| p.as_str())
        .ok_or_else(|| tool_error(&format!("Missing '{}' parameter", key)))
}

fn execute_scan_tool(args: &serde_json::Value) -> serde_json::Value {
    let path_str = match get_str_arg(args, "path") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let path = Path::new(path_str);
    if !path.exists() {
        return tool_error(&format!("File not found: {}", path_str));
    }

    let config = match HardgateConfig::load_or_default(None) {
        Ok(c) => c,
        Err(e) => return tool_error(&format!("Failed to load hardgate.toml: {}", e)),
    };
    let mut report = GateReport::new(config.gate.name.clone());
    let func_count = analyze_file_list(&[path.to_path_buf()], &config, Path::new("."), &mut report);

    report.finalize(1, func_count, 0);
    json!({ "content": [{ "type": "text", "text": report.render_agent() }] })
}

fn analyze_file_list(
    paths: &[PathBuf],
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) -> usize {
    let anti_gaming = AntiGamingScanner::new(&config.anti_gaming);
    let invariants = InvariantsChecker::new(&config.invariants.rules);
    let mut func_count = 0;

    for path in paths {
        let Ok(content) = fs::read_to_string(path) else {
            report
                .advisories
                .push(format!("Skipped unreadable file `{}`.", path.display()));
            continue;
        };
        let funcs = analyze_file_content(
            AnalyzeInput {
                path,
                content: &content,
                config,
                root,
                anti_gaming: &anti_gaming,
                invariants: &invariants,
            },
            report,
        );
        func_count += funcs.len();
    }

    func_count
}

fn execute_metrics_tool(args: &serde_json::Value) -> serde_json::Value {
    let path_str = match get_str_arg(args, "path") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let symbol_str = match get_str_arg(args, "symbol") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let Ok(content) = fs::read_to_string(path_str) else {
        return tool_error(&format!("Cannot open: {}", path_str));
    };

    let mut analyzer = ComplexityAnalyzer::new();
    let funcs = analyzer.analyze_file(Path::new(path_str), &content, Path::new("."));

    funcs
        .iter()
        .find(|m| m.name == symbol_str)
        .map(|f| json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(f).unwrap_or_default() }] }))
        .unwrap_or_else(|| tool_error(&format!("Symbol '{}' not found in {}", symbol_str, path_str)))
}

fn tool_error(msg: &str) -> serde_json::Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": msg }] })
}

fn send_success<W: Write>(
    out: &mut W,
    id: serde_json::Value,
    res: serde_json::Value,
) -> Result<()> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(res),
        error: None,
    };
    writeln!(out, "{}", serde_json::to_string(&resp)?)?;
    out.flush()?;
    Ok(())
}

fn send_parse_error<W: Write>(out: &mut W, err: serde_json::Error) -> Result<()> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: serde_json::Value::Null,
        result: None,
        error: Some(json!({ "code": -32700, "message": format!("Parse error: {}", err) })),
    };
    writeln!(out, "{}", serde_json::to_string(&resp)?)?;
    out.flush()?;
    Ok(())
}

fn send_invalid_request_error<W: Write>(out: &mut W, id: Option<serde_json::Value>) -> Result<()> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: id.unwrap_or(serde_json::Value::Null),
        result: None,
        error: Some(json!({ "code": -32600, "message": "Invalid Request: jsonrpc must be '2.0'" })),
    };
    writeln!(out, "{}", serde_json::to_string(&resp)?)?;
    out.flush()?;
    Ok(())
}

fn send_method_not_found<W: Write>(out: &mut W, id: serde_json::Value, method: &str) -> Result<()> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(json!({ "code": -32601, "message": format!("Method not found: {}", method) })),
    };
    writeln!(out, "{}", serde_json::to_string(&resp)?)?;
    out.flush()?;
    Ok(())
}
