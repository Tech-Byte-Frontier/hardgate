use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use std::io::Write;

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
}

pub(crate) fn success<W: Write>(
    out: &mut W,
    id: serde_json::Value,
    result: serde_json::Value,
) -> Result<()> {
    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(result),
        error: None,
    };
    write_response(out, response)
}

pub(crate) fn parse_error<W: Write>(out: &mut W, error: serde_json::Error) -> Result<()> {
    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: serde_json::Value::Null,
        result: None,
        error: Some(json!({ "code": -32700, "message": format!("Parse error: {error}") })),
    };
    write_response(out, response)
}

pub(crate) fn invalid_request<W: Write>(out: &mut W, id: Option<serde_json::Value>) -> Result<()> {
    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: id.unwrap_or(serde_json::Value::Null),
        result: None,
        error: Some(json!({
            "code": -32600,
            "message": "Invalid Request: jsonrpc must be '2.0'"
        })),
    };
    write_response(out, response)
}

pub(crate) fn method_not_found<W: Write>(
    out: &mut W,
    id: serde_json::Value,
    method: &str,
) -> Result<()> {
    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(json!({ "code": -32601, "message": format!("Method not found: {method}") })),
    };
    write_response(out, response)
}

fn write_response<W: Write>(out: &mut W, response: JsonRpcResponse) -> Result<()> {
    writeln!(out, "{}", serde_json::to_string(&response)?)?;
    out.flush()?;
    Ok(())
}
