//! Complexity fixtures for integration tests.

use hardgate::engines::complexity::FunctionMetrics;
use std::path::PathBuf;

/// High-complexity function fixture; `lines` doubles as statement count.
pub fn sample_metrics(lines: usize) -> FunctionMetrics {
    FunctionMetrics {
        test_only: false,
        size: None,
        name: "untested_monster".to_string(),
        file: PathBuf::from("src/calc.rs"),
        start_line: 1,
        start_column: 0,
        end_line: lines,
        lines,
        parameters: 2,
        cyclomatic: 10,
        max_nesting_depth: 3,
        statements: lines,
        cyclomatic_breakdown: Vec::new(),
    }
}
