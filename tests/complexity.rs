#[path = "support/metrics.rs"]
mod metrics;

use hardgate::config::FunctionBudgets;
use hardgate::engines::ComplexityAnalyzer;
use hardgate::engines::complexity::FunctionMetrics;
use std::path::Path;

/// Analyze one fixture file and return its single function.
fn analyze_one(path: &str, code: &str) -> FunctionMetrics {
    let mut analyzer = ComplexityAnalyzer::new();
    let found = analyzer.analyze_file(Path::new(path), code, Path::new("."));
    assert_eq!(found.len(), 1);
    found.into_iter().next().unwrap()
}

/// OneAnalyzer per language, driven by a table so the four cases share
/// a single assertion path instead of four cloned blocks.
#[test]
fn test_complexity_analyzers() {
    let rust_code = r#"
    fn complex_decision(a: i32, b: i32) -> i32 {
        if a > 0 && b > 0 {
            match a {
                1 => 10,
                2 => 20,
                _ => 30,
            }
        } else {
            0
        }
    }
    "#;
    let ts_code = r#"
    function classify(n: number): string {
        switch (n) {
            case 1:
                return "one";
            default:
                return "other";
        }
    }
    "#;
    let py_code = r#"
def calculate(data):
    total = 0
    for x in data:
        if x > 10:
            total += x
    return total
"#;
    let go_code = r#"
package main

func SumPositive(nums []int) int {
    sum := 0
    for _, n := range nums {
        if n > 0 {
            sum += n
        }
    }
    return sum
}
"#;

    // (path, code, expected name, expected params, min cyclomatic, min cognitive)
    let cases = [
        ("src/test.rs", rust_code, "complex_decision", 2, 4, 3),
        ("src/test.ts", ts_code, "classify", 1, 2, 2),
        ("src/test.py", py_code, "calculate", 1, 2, 0),
        ("src/test.go", go_code, "SumPositive", 1, 2, 0),
    ];
    for (path, code, name, params, cyclo, cog) in cases {
        let f = analyze_one(path, code);
        assert_eq!(f.name, name, "wrong symbol for {path}");
        assert_eq!(f.parameters, params, "wrong arity for {name}");
        assert!(f.cyclomatic >= cyclo, "low cyclomatic for {name}");
        assert!(f.cognitive >= cog, "low cognitive for {name}");
    }
}

#[test]
fn test_complexity_ast_breakdown() {
    let code = r#"
    fn nested_decisions(x: i32, y: i32) -> i32 {
        if x > 0 {
            if y > 0 {
                1
            } else {
                2
            }
        } else {
            0
        }
    }
    "#;

    let f = analyze_one("src/test.rs", code);

    // Both the outer if and the nested if (higher score) must be reported.
    assert!(!f.cognitive_breakdown.is_empty());
    assert!(f.cognitive_breakdown.iter().any(|c| c.score >= 2));
    assert!(!f.cyclomatic_breakdown.is_empty());

    let budgets = FunctionBudgets {
        max_cognitive: Some(1),
        ..Default::default()
    };
    let violations = ComplexityAnalyzer::check_violations(std::slice::from_ref(&f), &budgets);
    assert_eq!(violations.len(), 1);
    assert!(!violations[0].breakdown.is_empty());
    assert!(violations[0].breakdown[0].score >= 2);
}

#[test]
fn test_complexity_advanced_budgets_enforced() {
    let budgets = FunctionBudgets {
        max_halstead_difficulty: Some(10.0),
        max_statements: Some(5),
        max_abc: Some(10.0),
        ..Default::default()
    };
    let bad = metrics::sample_metrics(99, 2, 99.0, 99.0);
    let bad_violations = ComplexityAnalyzer::check_violations(&[bad], &budgets);
    let reported: Vec<&str> = bad_violations.iter().map(|v| v.metric.as_str()).collect();
    for expected in ["Halstead Difficulty", "Statement Count", "ABC Score"] {
        assert!(reported.contains(&expected), "missing {expected}");
    }

    let good = metrics::sample_metrics(1, 2, 1.0, 1.0);
    assert!(ComplexityAnalyzer::check_violations(&[good], &budgets).is_empty());
}
