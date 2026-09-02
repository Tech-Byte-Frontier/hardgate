use crate::config::MutationConfig;
use crate::engines::complexity::SupportedLanguage;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tree_sitter::Node;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationViolation {
    pub report_file: PathBuf,
    pub metric: String,
    pub actual: f64,
    pub limit: f64,
    pub message: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MutationStats {
    pub killed: usize,
    pub survived: usize,
    pub timeout: usize,
    pub unviable: usize,
    pub total: usize,
}

impl MutationStats {
    pub fn score_percent(&self) -> f64 {
        let viable = self.killed + self.survived;
        if viable == 0 {
            100.0
        } else {
            (self.killed as f64 / viable as f64) * 100.0
        }
    }
}

pub struct MutationGatekeeper {
    config: MutationConfig,
}

impl MutationGatekeeper {
    pub fn new(config: &MutationConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    pub fn evaluate_report(&self, report_path: &Path) -> anyhow::Result<Vec<MutationViolation>> {
        let mut violations = Vec::new();
        let content = fs::read_to_string(report_path)?;
        let json_val: serde_json::Value = serde_json::from_str(&content)?;

        let stats = parse_mutation_json(&json_val);
        let score = stats.score_percent();
        let min_score = self.config.min_score.unwrap_or(85.0);

        if score < min_score {
            violations.push(MutationViolation {
                report_file: report_path.to_path_buf(),
                metric: "Mutation Kill Rate".to_string(),
                actual: score,
                limit: min_score,
                message: format!(
                    "Mutation testing score {:.1}% is below floor {:.1}% (Killed: {}, Survived: {})",
                    score, min_score, stats.killed, stats.survived
                ),
                recommendation: "Write semantic assertions to catch mutant faults.".to_string(),
            });
        }

        if self.config.reject_timeouts && stats.timeout > 0 {
            violations.push(MutationViolation {
                report_file: report_path.to_path_buf(),
                metric: "Mutation Timeouts".to_string(),
                actual: stats.timeout as f64,
                limit: 0.0,
                message: format!("Mutation run had {} timed-out mutants.", stats.timeout),
                recommendation: "Investigate and resolve infinite loops in test runs.".to_string(),
            });
        }

        Ok(violations)
    }
}

fn parse_mutation_json(val: &serde_json::Value) -> MutationStats {
    if let Some(stats) = parse_stryker_json(val) {
        return stats;
    }
    if let Some(stats) = parse_cargo_mutants_json(val) {
        return stats;
    }
    parse_generic_mutation_json(val)
}

fn parse_stryker_json(val: &serde_json::Value) -> Option<MutationStats> {
    let files = val.get("files")?.as_object()?;
    let mut stats = MutationStats::default();

    for file_val in files.values() {
        if let Some(mutants) = file_val.get("mutants").and_then(|m| m.as_array()) {
            accumulate_stryker_mutants(mutants, &mut stats);
        }
    }

    Some(stats)
}

fn accumulate_stryker_mutants(mutants: &[serde_json::Value], stats: &mut MutationStats) {
    for m in mutants {
        stats.total += 1;
        match m.get("status").and_then(|s| s.as_str()) {
            Some("Killed") => stats.killed += 1,
            Some("Survived") => stats.survived += 1,
            Some("Timeout") => stats.timeout += 1,
            _ => stats.unviable += 1,
        }
    }
}

fn parse_cargo_mutants_json(val: &serde_json::Value) -> Option<MutationStats> {
    let mutants = val.get("outcomes")?.as_array()?;
    let mut stats = MutationStats::default();

    for m in mutants {
        stats.total += 1;
        let summary = m.get("summary").and_then(|s| s.as_str()).unwrap_or("");
        if summary == "caught" {
            stats.killed += 1;
        } else if summary == "missed" {
            stats.survived += 1;
        } else if summary == "timeout" {
            stats.timeout += 1;
        } else {
            stats.unviable += 1;
        }
    }

    Some(stats)
}

fn parse_generic_mutation_json(val: &serde_json::Value) -> MutationStats {
    let mut stats = MutationStats::default();
    if let Some(k) = val.get("killed").and_then(|v| v.as_u64()) {
        stats.killed = k as usize;
    }
    if let Some(s) = val.get("survived").and_then(|v| v.as_u64()) {
        stats.survived = s as usize;
    }
    if let Some(t) = val.get("timeout").and_then(|v| v.as_u64()) {
        stats.timeout = t as usize;
    }
    stats.total = stats.killed + stats.survived + stats.timeout + stats.unviable;
    stats
}

// ─────────────────────────────────────────────────────────────────────────────
// Native AST Mutation Generator & Runner
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstMutant {
    pub id: usize,
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub original: String,
    pub replacement: String,
    pub description: String,
}

pub struct AstMutationGenerator;

impl Default for AstMutationGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl AstMutationGenerator {
    pub fn new() -> Self {
        Self
    }

    pub fn generate_mutants(&mut self, path: &Path, content: &str) -> Vec<AstMutant> {
        let Some((_lang, tree)) = SupportedLanguage::parse_file(path, content) else {
            return Vec::new();
        };

        let mut mutants = Vec::new();
        collect_ast_mutants(tree.root_node(), content.as_bytes(), path, &mut mutants);
        mutants
    }
}

fn collect_ast_mutants(node: Node, source: &[u8], path: &Path, mutants: &mut Vec<AstMutant>) {
    if node.kind() == "binary_expression" {
        collect_binary_mutants(node, source, path, mutants);
    } else if let Some(m) = try_mutate_boolean(node, source, path, mutants.len() + 1) {
        mutants.push(m);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ast_mutants(child, source, path, mutants);
    }
}

fn collect_binary_mutants(node: Node, source: &[u8], path: &Path, mutants: &mut Vec<AstMutant>) {
    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        let Ok(op_text) = child.utf8_text(source) else {
            continue;
        };
        if let Some(rep) = invert_binary_op(op_text) {
            let id = mutants.len() + 1;
            let line = child.start_position().row + 1;
            let column = child.start_position().column + 1;
            mutants.push(AstMutant {
                id,
                file: path.to_path_buf(),
                line,
                column,
                start_byte: child.start_byte(),
                end_byte: child.end_byte(),
                original: op_text.to_string(),
                replacement: rep.to_string(),
                description: format!("Replace `{}` with `{}`", op_text, rep),
            });
        }
    }
}

const BINARY_MUTATIONS: &[(&str, &str)] = &[
    ("==", "!="),
    ("!=", "=="),
    ("<", ">="),
    ("<=", ">"),
    (">", "<="),
    (">=", "<"),
    ("&&", "||"),
    ("||", "&&"),
    ("+", "-"),
    ("-", "+"),
    ("*", "/"),
    ("/", "*"),
];

fn invert_binary_op(op: &str) -> Option<&'static str> {
    for &(original, mutated) in BINARY_MUTATIONS {
        if original == op {
            return Some(mutated);
        }
    }
    None
}

fn try_mutate_boolean(node: Node, source: &[u8], path: &Path, id: usize) -> Option<AstMutant> {
    let kind = node.kind();
    if kind != "boolean_literal" && kind != "true" && kind != "false" {
        return None;
    }
    let text = node.utf8_text(source).ok()?;
    let replacement = match text {
        "true" => "false",
        "false" => "true",
        _ => return None,
    };
    Some(AstMutant {
        id,
        file: path.to_path_buf(),
        line: node.start_position().row + 1,
        column: node.start_position().column + 1,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        original: text.to_string(),
        replacement: replacement.to_string(),
        description: format!("Replace boolean `{}` with `{}`", text, replacement),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutantOutcome {
    Killed,
    Survived,
    Timeout,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutantExecutionResult {
    pub mutant: AstMutant,
    pub outcome: MutantOutcome,
    pub duration_ms: u128,
    pub command: String,
}

pub struct NativeMutationRunner {
    timeout_secs: u64,
    test_cmd: Option<String>,
}

struct RollbackGuard<'a> {
    file_path: &'a Path,
    original_bytes: &'a [u8],
}

impl<'a> Drop for RollbackGuard<'a> {
    fn drop(&mut self) {
        let _ = fs::write(self.file_path, self.original_bytes);
    }
}

impl NativeMutationRunner {
    pub fn new(timeout_secs: u64, test_cmd: Option<String>) -> Self {
        Self {
            timeout_secs,
            test_cmd,
        }
    }

    pub fn run_mutant(&self, mutant: &AstMutant, root: &Path) -> MutantExecutionResult {
        let start = Instant::now();
        let target_path = if mutant.file.is_absolute() {
            mutant.file.clone()
        } else {
            root.join(&mutant.file)
        };

        let Ok(original_bytes) = fs::read(&target_path) else {
            return MutantExecutionResult {
                mutant: mutant.clone(),
                outcome: MutantOutcome::Error,
                duration_ms: 0,
                command: "".to_string(),
            };
        };

        let cmd_str = self.resolve_test_command(&mutant.file, root);

        // Scope the file modification with RAII rollback guard
        let outcome = {
            let _guard = RollbackGuard {
                file_path: &target_path,
                original_bytes: &original_bytes,
            };

            let mut mutated_bytes = Vec::new();
            if mutant.start_byte <= original_bytes.len() && mutant.end_byte <= original_bytes.len()
            {
                mutated_bytes.extend_from_slice(&original_bytes[..mutant.start_byte]);
                mutated_bytes.extend_from_slice(mutant.replacement.as_bytes());
                mutated_bytes.extend_from_slice(&original_bytes[mutant.end_byte..]);
                if fs::write(&target_path, &mutated_bytes).is_err() {
                    return MutantExecutionResult {
                        mutant: mutant.clone(),
                        outcome: MutantOutcome::Error,
                        duration_ms: 0,
                        command: cmd_str,
                    };
                }
            } else {
                return MutantExecutionResult {
                    mutant: mutant.clone(),
                    outcome: MutantOutcome::Error,
                    duration_ms: 0,
                    command: cmd_str,
                };
            }

            self.execute_test_with_timeout(&cmd_str, root)
        };

        let duration_ms = start.elapsed().as_millis();
        MutantExecutionResult {
            mutant: mutant.clone(),
            outcome,
            duration_ms,
            command: cmd_str,
        }
    }

    fn resolve_test_command(&self, file: &Path, root: &Path) -> String {
        if let Some(ref cmd) = self.test_cmd {
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            return cmd
                .replace("{file}", &file.to_string_lossy())
                .replace("{stem}", stem);
        }

        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        match ext {
            "rs" => resolve_rust_test_cmd(stem),
            "ts" | "tsx" | "js" | "jsx" => resolve_js_test_cmd(file, root, stem, ext),
            _ => "cargo test".to_string(),
        }
    }

    fn execute_test_with_timeout(&self, cmd_str: &str, root: &Path) -> MutantOutcome {
        let tokens: Vec<&str> = cmd_str.split_whitespace().collect();
        if tokens.is_empty() {
            return MutantOutcome::Error;
        }

        let program = tokens[0];
        let args = &tokens[1..];

        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.current_dir(root);
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        let Ok(mut child) = cmd.spawn() else {
            return MutantOutcome::Error;
        };

        let start = Instant::now();
        let max_duration = Duration::from_secs(self.timeout_secs);

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() {
                        MutantOutcome::Survived
                    } else {
                        MutantOutcome::Killed
                    };
                }
                Ok(None) => {
                    if start.elapsed() > max_duration {
                        let _ = child.kill();
                        let _ = child.wait();
                        return MutantOutcome::Timeout;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return MutantOutcome::Error,
            }
        }
    }
}

fn resolve_rust_test_cmd(stem: &str) -> String {
    if !stem.is_empty() && stem != "main" && stem != "lib" && stem != "mod" {
        format!("cargo test {}", stem)
    } else {
        "cargo test".to_string()
    }
}

fn resolve_js_test_cmd(file: &Path, root: &Path, stem: &str, ext: &str) -> String {
    let test_candidates = [
        format!("{}.test.{}", stem, ext),
        format!("{}.spec.{}", stem, ext),
    ];
    for cand in &test_candidates {
        if root.join("tests").join(cand).exists() || file.with_file_name(cand).exists() {
            return format!("pnpm test {}", cand);
        }
    }
    "pnpm test".to_string()
}
