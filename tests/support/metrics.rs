//! Complexity fixtures for integration tests.

use hardgate::engines::complexity::FunctionMetrics;
use std::path::PathBuf;

/// High-complexity function fixture; `lines` doubles as statement count.
pub fn sample_metrics(lines: usize, cognitive: u32, halstead: f64, abc: f64) -> FunctionMetrics {
    FunctionMetrics {
        name: "untested_monster".to_string(),
        file: PathBuf::from("src/calc.rs"),
        start_line: 1,
        end_line: lines,
        lines,
        parameters: 2,
        cyclomatic: 10,
        cognitive,
        halstead_difficulty: halstead,
        max_nesting_depth: 3,
        statements: lines,
        abc_score: abc,
        cognitive_breakdown: Vec::new(),
        cyclomatic_breakdown: Vec::new(),
    }
}
