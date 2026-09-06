//! Cargo's documented JSON compiler-message contract; no terminal-text parsing.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDiagnostic {
    pub tool: String,
    pub rule: String,
    pub level: String,
    pub message: String,
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub package_id: String,
    pub target: Value,
    #[serde(default)]
    pub targets: Vec<Value>,
    pub blocking: bool,
}

pub(crate) struct CargoDiagnostics {
    pub findings: Vec<ToolDiagnostic>,
    pub complete: bool,
    pub success: Option<bool>,
}

pub(crate) fn is_clippy(tokens: &[String]) -> bool {
    tokens.first().is_some_and(|program| {
        Path::new(program)
            .file_name()
            .is_some_and(|name| name == "cargo")
    }) && tokens
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "clippy")
}

pub(crate) fn structured_tokens(tokens: &mut Vec<String>) {
    if !is_clippy(tokens) {
        return;
    }
    let boundary = tokens
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(tokens.len());
    let mut prefix = tokens.drain(..boundary).peekable();
    let mut rewritten = Vec::new();
    while let Some(arg) = prefix.next() {
        if arg == "--message-format" {
            prefix.next();
        } else if !arg.starts_with("--message-format=") {
            rewritten.push(arg);
        }
    }
    drop(prefix);
    rewritten.push("--message-format=json".into());
    rewritten.append(tokens);
    *tokens = rewritten;
}

pub(crate) fn parse(output: &str, root: &Path) -> CargoDiagnostics {
    let mut result = CargoDiagnostics {
        findings: Vec::new(),
        complete: !output.contains("[output truncated"),
        success: None,
    };
    for line in output.lines().filter(|line| line.starts_with('{')) {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            result.complete = false;
            continue;
        };
        match record["reason"].as_str() {
            Some("build-finished") => result.success = record["success"].as_bool(),
            Some("compiler-message") => collect_message(&record, root, &mut result.findings),
            _ => {}
        }
    }
    result.complete &= result.success.is_some();
    result
}

fn collect_message(record: &Value, root: &Path, findings: &mut Vec<ToolDiagnostic>) {
    let message = &record["message"];
    let Some(level @ ("error" | "warning")) = message["level"].as_str() else {
        return;
    };
    let Some(span) = message["spans"]
        .as_array()
        .and_then(|spans| spans.iter().find(|span| span["is_primary"] == true))
    else {
        return;
    };
    let Some(file) = span["file_name"].as_str() else {
        return;
    };
    let file = root.join(file);
    let rule = message["code"]["code"].as_str().unwrap_or("rustc");
    let finding = ToolDiagnostic {
        tool: if rule.starts_with("clippy::") {
            "clippy"
        } else {
            "rustc"
        }
        .into(),
        rule: rule.into(),
        level: level.into(),
        message: message["message"].as_str().unwrap_or_default().into(),
        file,
        line: span["line_start"].as_u64().unwrap_or(0) as usize,
        column: span["column_start"].as_u64().unwrap_or(0) as usize,
        end_line: span["line_end"].as_u64().unwrap_or(0) as usize,
        package_id: record["package_id"].as_str().unwrap_or_default().into(),
        target: record["target"].clone(),
        targets: vec![record["target"].clone()],
        blocking: level == "error",
    };
    // Cargo can relay the same finding from several targets; preserve one review
    // item for each rule/location/message and retain all observed target scopes.
    if let Some(old) = findings.iter_mut().find(|old| {
        old.rule == finding.rule
            && old.package_id == finding.package_id
            && old.file == finding.file
            && old.line == finding.line
            && old.column == finding.column
            && old.message == finding.message
            && old.level == finding.level
    }) {
        if !old.targets.contains(&finding.target) {
            old.targets.push(finding.target);
        }
    } else {
        findings.push(finding);
    }
}
