use crate::commands::{AnalyzeInput, analyze_file_content};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{DiscoverOptions, discover_files_with_exclusions};
use crate::engines::{AntiGamingScanner, CloneDetector, ComplexityAnalyzer, InvariantsChecker};
use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
mod response;

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
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
            if reader.read_line(&mut hdr)? == 0 {
                return Err(anyhow!("Unexpected EOF in MCP Content-Length headers"));
            }
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
        Err(e) => return response::parse_error(out, e),
    };

    if request.jsonrpc != "2.0" {
        return response::invalid_request(out, request.id);
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
        return response::success(out, id, res);
    }
    if req.method == "ping" {
        return response::success(out, id, json!({}));
    }
    if req.method == "tools/list" {
        return response::success(out, id, get_tools_list());
    }
    if req.method == "tools/call" {
        let res = handle_tool_call(req.params.as_ref());
        return response::success(out, id, res);
    }
    if req.method == "notifications/initialized" {
        return Ok(());
    }

    response::method_not_found(out, id, &req.method)
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
                        },
                        "diff": {
                            "type": "boolean",
                            "description": "Check only Git-modified files; requires a Git worktree."
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
    let Some(name) = params.get("name").and_then(|n| n.as_str()) else {
        return tool_error("Missing 'name' parameter in tool call");
    };
    let args = match tool_arguments(params) {
        Ok(value) => value,
        Err(error) => return tool_error(error),
    };
    dispatch_tool(name, &args)
}

fn tool_arguments(params: &serde_json::Value) -> Result<serde_json::Value, &'static str> {
    match params.get("arguments") {
        None => Ok(json!({})),
        Some(value) if value.is_object() => Ok(value.clone()),
        Some(_) => Err("Tool call 'arguments' must be an object"),
    }
}

fn dispatch_tool(name: &str, args: &serde_json::Value) -> serde_json::Value {
    match name {
        "hardgate_check" => execute_check_tool(args),
        "hardgate_scan_file" => execute_scan_tool(args),
        "hardgate_get_metrics" => execute_metrics_tool(args),
        _ => tool_error(&format!("Unknown tool: {}", name)),
    }
}
fn execute_check_tool(args: &serde_json::Value) -> serde_json::Value {
    let config = match load_config() {
        Ok(c) => c,
        Err(error) => return error,
    };
    execute_check_with_config(args, &config)
}

fn execute_check_with_config(
    args: &serde_json::Value,
    config: &HardgateConfig,
) -> serde_json::Value {
    let root = Path::new(".");
    let diff_only = match parse_bool_arg(args, "diff") {
        Ok(value) => value,
        Err(error) => return tool_error(&error),
    };
    let paths = match paths_arg(args) {
        Ok(paths) => paths,
        Err(error) => return tool_error(&error),
    };
    if let Some(arr) = paths {
        return execute_scoped_check(arr, config, root);
    }
    execute_discovered_check(config, root, diff_only)
}

fn load_config() -> Result<HardgateConfig, serde_json::Value> {
    HardgateConfig::load_or_default(None)
        .map_err(|error| tool_error(&format!("Failed to load hardgate.toml: {error}")))
}

fn paths_arg(args: &serde_json::Value) -> Result<Option<&[serde_json::Value]>, String> {
    match args.get("paths") {
        None => Ok(None),
        Some(value) => value
            .as_array()
            .map(|paths| Some(paths.as_slice()))
            .ok_or_else(|| "Parameter 'paths' must be an array of strings".to_string()),
    }
}

fn execute_discovered_check(
    config: &HardgateConfig,
    root: &Path,
    diff_only: bool,
) -> serde_json::Value {
    let discovery = match discover_files_with_exclusions(DiscoverOptions {
        root,
        diff_only,
        exclusions: &config.budgets.files.exclusions.paths,
    }) {
        Ok(result) => result,
        Err(error) => return tool_error(&format!("Failed to discover source files: {error}")),
    };
    if discovery.files.is_empty() {
        return tool_error("No source files discovered; refusing an empty successful check");
    }
    finish_check_response(CheckResponseInput {
        target_files: &discovery.files,
        excluded_count: discovery.excluded_files.len(),
        config,
        root,
        extra_advisories: Vec::new(),
    })
}
fn execute_scoped_check(
    arr: &[serde_json::Value],
    config: &HardgateConfig,
    root: &Path,
) -> serde_json::Value {
    let (files, missing) = match partition_existing_paths(arr) {
        Ok(result) => result,
        Err(error) => return tool_error(&error),
    };
    if !missing.is_empty() {
        return tool_error(&format!("Files not found: {}", missing.join(", ")));
    }
    if files.is_empty() {
        return tool_error("No paths provided; refusing an empty successful check");
    }
    finish_check_response(CheckResponseInput {
        target_files: &files,
        excluded_count: 0,
        config,
        root,
        extra_advisories: Vec::new(),
    })
}
fn partition_existing_paths(
    arr: &[serde_json::Value],
) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    let mut files = Vec::new();
    let mut missing = Vec::new();
    for value in arr {
        let Some(p) = value.as_str() else {
            return Err("Parameter 'paths' must contain only strings".to_string());
        };
        if p.is_empty() {
            return Err("Parameter 'paths' must not contain empty paths".to_string());
        }
        let pb = PathBuf::from(p);
        if pb.exists() {
            files.push(pb);
        } else {
            missing.push(p.to_string());
        }
    }
    Ok((files, missing))
}
fn parse_bool_arg(args: &serde_json::Value, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("Parameter '{key}' must be a boolean")),
    }
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
    let read_results = match read_files_content(target_files) {
        Ok(contents) => contents,
        Err(error) => return tool_error(&error),
    };
    let func_count = analyze_file_contents(&read_results, config, root, &mut report);
    append_clone_results(&read_results, config, root, &mut report);

    report.finalize(target_files.len(), func_count, 0);
    json!({ "content": [{ "type": "text", "text": report.render_agent() }] })
}

fn read_files_content(paths: &[PathBuf]) -> Result<Vec<(PathBuf, String)>, String> {
    let mut contents = Vec::with_capacity(paths.len());
    let mut failures = Vec::new();
    for path in paths {
        match fs::read_to_string(path) {
            Ok(content) => contents.push((path.clone(), content)),
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }
    if failures.is_empty() {
        Ok(contents)
    } else {
        Err(format!(
            "Unable to read required source file(s): {}",
            failures.join("; ")
        ))
    }
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
    execute_scan_path(path)
}

fn execute_scan_path(path: &Path) -> serde_json::Value {
    let config = match load_config() {
        Ok(config) => config,
        Err(error) => return error,
    };
    let mut report = GateReport::new(config.gate.name.clone());
    let read_results = match read_files_content(&[path.to_path_buf()]) {
        Ok(contents) => contents,
        Err(error) => return tool_error(&error),
    };
    let func_count = analyze_file_contents(&read_results, &config, Path::new("."), &mut report);

    report.finalize(1, func_count, 0);
    json!({ "content": [{ "type": "text", "text": report.render_agent() }] })
}

fn analyze_file_contents(
    files: &[(PathBuf, String)],
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) -> usize {
    let anti_gaming = AntiGamingScanner::new(&config.anti_gaming);
    let invariants = InvariantsChecker::new(&config.invariants.rules);
    let mut func_count = 0;

    for (path, content) in files {
        let funcs = analyze_file_content(
            AnalyzeInput {
                path,
                content,
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

    let Some(function) = funcs.iter().find(|m| m.name == symbol_str) else {
        return tool_error(&format!(
            "Symbol '{}' not found in {}",
            symbol_str, path_str
        ));
    };
    match serde_json::to_string_pretty(function) {
        Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
        Err(error) => tool_error(&format!(
            "Failed to serialize metrics for '{}' in {}: {error}",
            symbol_str, path_str
        )),
    }
}

fn tool_error(msg: &str) -> serde_json::Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": msg }] })
}
