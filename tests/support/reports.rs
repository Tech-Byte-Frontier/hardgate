//! Shared gate-report fixtures for rendering tests.

use hardgate::GateReport;
use hardgate::engines::{BudgetViolation, ComplexityViolation};
use std::path::PathBuf;

/// Failing report with one complexity and one file-budget violation.
pub fn failing_report() -> GateReport {
    let mut report = GateReport::new("demo".to_string());
    report.complexity_violations.push(ComplexityViolation {
        file: PathBuf::from("src/main.rs"),
        function_name: "login".to_string(),
        line_number: 1,
        end_line: 1,
        metric: "Cyclomatic Complexity".to_string(),
        actual: 18.0,
        limit: 10.0,
        breakdown: vec![],
        message: "too complex".to_string(),
        recommendation: "Split `login` into helpers.".to_string(),
    });
    report.budget_violations.push(BudgetViolation {
        file: PathBuf::from("src/big.rs"),
        metric: "max_lines".to_string(),
        actual: 600,
        limit: 400,
        message: "file too large".to_string(),
    });
    report
}
