use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{DiscoverOptions, discover_files_with_exclusions};
use crate::engines::{
    AntiGamingScanner, ComplexityAnalyzer, InvariantsChecker, check_file_budgets,
};
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

pub fn run_mcp_server() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            process_mcp_line(trimmed, &mut stdout)?;
        }
    }

    Ok(())
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
    let config = HardgateConfig::load_or_default(None).unwrap_or_default();
    let root = Path::new(".");
    let (target_files, excluded_count) =
        if let Some(arr) = args.get("paths").and_then(|p| p.as_array()) {
            let files = arr
                .iter()
                .filter_map(|p| p.as_str().map(PathBuf::from))
                .collect();
            (files, 0)
        } else {
            let discovery = discover_files_with_exclusions(DiscoverOptions {
                root,
                diff_only: false,
                exclusions: &config.budgets.files.exclusions.paths,
            })
            .unwrap_or_default();
            (discovery.files, discovery.excluded_files.len())
        };

    let mut report = GateReport::new(config.gate.name.clone());
    if excluded_count > 0 {
        let noun = if excluded_count == 1 { "file" } else { "files" };
        report.advisories.push(format!(
            "{} {} excluded from file budget checks via hardgate.toml.",
            excluded_count, noun
        ));
    }
    let func_count = analyze_file_list(&target_files, &config, root, &mut report);

    report.finalize(target_files.len(), func_count, 0);
    json!({ "content": [{ "type": "text", "text": report.render_agent() }] })
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

    let config = HardgateConfig::load_or_default(None).unwrap_or_default();
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
            continue;
        };
        report
            .budget_violations
            .extend(check_file_budgets(path, &config.budgets.files, root));
        if config.anti_gaming.disallow_suppressions {
            report
                .suppression_violations
                .extend(anti_gaming.scan_content(path, &content, root));
        }
        if config.invariants.enforce {
            report
                .invariant_violations
                .extend(invariants.check_file(path, &content, root));
        }
        let mut analyzer = ComplexityAnalyzer::new();
        let funcs = analyzer.analyze_file(path, &content, root);
        func_count += funcs.len();
        report
            .complexity_violations
            .extend(ComplexityAnalyzer::check_violations(
                &funcs,
                &config.budgets.functions,
            ));
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
