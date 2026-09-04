use super::{GateReport, push_gate_header, status_label};
use colored::*;
use serde::{Deserialize, Serialize};

/// Machine-readable rollup of a [`GateReport`]: per-category counts plus
/// scan totals. Embedded in full JSON output and returned alone by
/// [`GateReport::render_summary_json`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GateSummary {
    pub total_errors: usize,
    pub clones: usize,
    pub ast_violations: usize,
    pub complexity: usize,
    pub file_budgets: usize,
    pub suppressions: usize,
    pub architecture: usize,
    pub coverage: usize,
    pub mutation: usize,
    pub dead_code: usize,
    pub tool: usize,
    pub files_scanned: usize,
    pub functions_analyzed: usize,
    pub files_with_violations: usize,
    pub passed: bool,
}

/// One entry of the "top files with violations" ranking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopFileEntry {
    pub file: String,
    pub violations: usize,
}

impl GateReport {
    /// Build the [`GateSummary`] rollup for this report.
    pub fn summary(&self) -> GateSummary {
        GateSummary {
            total_errors: self.total_violations(),
            clones: self.clone_violations.len(),
            ast_violations: self.complexity_violations.len(),
            complexity: self.complexity_violations.len(),
            file_budgets: self.budget_violations.len(),
            suppressions: self.suppression_violations.len(),
            architecture: self.invariant_violations.len(),
            coverage: self.coverage_violations.len(),
            mutation: self.mutation_violations.len(),
            dead_code: self.dead_code_violations.len(),
            tool: self.orchestration_violations.len(),
            files_scanned: self.files_scanned,
            functions_analyzed: self.functions_analyzed,
            files_with_violations: self.files_with_violations(),
            passed: self.passed,
        }
    }

    fn files_with_violations(&self) -> usize {
        use std::collections::HashSet;
        let mut files = HashSet::new();
        for v in &self.budget_violations {
            files.insert(v.file.to_string_lossy().to_string());
        }
        for v in &self.suppression_violations {
            files.insert(v.file.to_string_lossy().to_string());
        }
        for v in &self.complexity_violations {
            files.insert(v.file.to_string_lossy().to_string());
        }
        for v in &self.invariant_violations {
            files.insert(v.file.to_string_lossy().to_string());
        }
        for v in &self.clone_violations {
            files.insert(v.file_a.to_string_lossy().to_string());
            files.insert(v.file_b.to_string_lossy().to_string());
        }
        for v in &self.coverage_violations {
            files.insert(v.file.to_string_lossy().to_string());
        }
        for v in &self.dead_code_violations {
            files.insert(v.file.to_string_lossy().to_string());
        }
        // Mutation + orchestration violations reference reports/tools rather
        // than source files, so they don't contribute to the file count.
        files.len()
    }

    /// Top `limit` files ordered by violation count (descending, then path).
    pub fn top_files(&self, limit: usize) -> Vec<TopFileEntry> {
        use std::collections::HashMap;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for key in self.violation_file_keys() {
            *counts.entry(key).or_insert(0) += 1;
        }
        let mut entries: Vec<TopFileEntry> = counts
            .into_iter()
            .map(|(file, violations)| TopFileEntry { file, violations })
            .collect();
        entries.sort_by(|a, b| {
            b.violations
                .cmp(&a.violations)
                .then_with(|| a.file.cmp(&b.file))
        });
        entries.truncate(limit);
        entries
    }

    fn violation_file_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        push_keys(&mut keys, self.budget_violations.iter().map(|v| &v.file));
        push_keys(
            &mut keys,
            self.suppression_violations.iter().map(|v| &v.file),
        );
        push_keys(
            &mut keys,
            self.complexity_violations.iter().map(|v| &v.file),
        );
        push_keys(&mut keys, self.invariant_violations.iter().map(|v| &v.file));
        for v in &self.clone_violations {
            keys.push(v.file_a.to_string_lossy().to_string());
            keys.push(v.file_b.to_string_lossy().to_string());
        }
        push_keys(&mut keys, self.coverage_violations.iter().map(|v| &v.file));
        push_keys(&mut keys, self.dead_code_violations.iter().map(|v| &v.file));
        keys
    }

    /// Concise overview only: totals per category plus top offending files.
    /// Designed for CI logs, `jq`, and agent phase-by-phase filtering.
    pub fn render_summary(&self) -> String {
        let mut out = String::new();
        let s = self.summary();
        push_gate_header(
            &mut out,
            &self.gate_name,
            status_label(self.passed, s.total_errors),
        );
        out.push_str(&format!(
            "Summary: {} errors ({} clones, {} AST violations across {} files)\n",
            s.total_errors, s.clones, s.ast_violations, s.files_with_violations,
        ));
        out.push_str(&format!(
            "Scanned: {} files, {} functions in {}ms\n",
            self.files_scanned, self.functions_analyzed, self.duration_ms
        ));
        out.push_str(&format!(
            "Breakdown: clones={}, complexity={}, file-budget={}, anti-gaming={}, architecture={}, coverage={}, mutation={}, dead-code={}, tool={}\n",
            s.clones,
            s.complexity,
            s.file_budgets,
            s.suppressions,
            s.architecture,
            s.coverage,
            s.mutation,
            s.dead_code,
            s.tool,
        ));
        let top = self.top_files(10);
        if !top.is_empty() {
            out.push_str("Top files with violations:\n");
            for entry in &top {
                out.push_str(&format!("  - {} ({})\n", entry.file, entry.violations));
            }
        }
        for advisory in &self.advisories {
            out.push_str(&format!("{} {}\n", "warning:".yellow().bold(), advisory));
        }
        out.push_str(&format!(
            "{}\nresult: {}\n",
            "-".repeat(70).dimmed(),
            status_label(self.passed, s.total_errors),
        ));
        out
    }

    /// Full machine-readable JSON: every violation plus `summary` and
    /// `top_files` for `jq`-friendly CI and agent consumption.
    pub fn render_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&FullJson {
            report: self,
            summary: self.summary(),
            top_files: self.top_files(10),
        })
    }

    /// Lean summary-only JSON for `--summary --format json`: counts plus top
    /// files without the full per-violation payloads.
    pub fn render_summary_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&SummaryJson {
            gate_name: &self.gate_name,
            passed: self.passed,
            files_scanned: self.files_scanned,
            functions_analyzed: self.functions_analyzed,
            duration_ms: self.duration_ms,
            summary: self.summary(),
            top_files: self.top_files(10),
            advisories: &self.advisories,
        })
    }
}

#[derive(Serialize)]
struct FullJson<'a> {
    #[serde(flatten)]
    report: &'a GateReport,
    summary: GateSummary,
    top_files: Vec<TopFileEntry>,
}

#[derive(Serialize)]
struct SummaryJson<'a> {
    gate_name: &'a str,
    passed: bool,
    files_scanned: usize,
    functions_analyzed: usize,
    duration_ms: u128,
    summary: GateSummary,
    top_files: Vec<TopFileEntry>,
    advisories: &'a [String],
}

fn push_keys<'a>(keys: &mut Vec<String>, files: impl Iterator<Item = &'a std::path::PathBuf>) {
    for f in files {
        keys.push(f.to_string_lossy().to_string());
    }
}
