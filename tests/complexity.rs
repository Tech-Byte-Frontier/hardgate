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

#[test]
fn test_limit_violation_details_and_order_are_preserved() {
    let mut function = metrics::sample_metrics(99, 2, 99.0, 99.0);
    function.parameters = 6;
    function.max_nesting_depth = 6;
    let budgets = FunctionBudgets {
        max_parameters: Some(4),
        max_lines: Some(80),
        max_nesting_depth: Some(4),
        max_halstead_difficulty: Some(80.0),
        max_statements: Some(30),
        max_abc: Some(10.0),
        ..Default::default()
    };
    let violations =
        ComplexityAnalyzer::check_violations(std::slice::from_ref(&function), &budgets);
    let expected = [
        (
            "Parameter Count",
            6.0,
            4.0,
            "Function has 6 parameters (budget: 4)",
            "Introduce a config struct or parameter object for `untested_monster`.",
        ),
        (
            "Function Lines",
            99.0,
            80.0,
            "Function body spans 99 lines (budget: 80)",
            "Split `untested_monster` into smaller focused functions.",
        ),
        (
            "Nesting Depth",
            6.0,
            4.0,
            "Max nesting depth is 6 (budget: 4)",
            "Use early returns or guard clauses to reduce nesting depth in `untested_monster`.",
        ),
        (
            "Halstead Difficulty",
            99.0,
            80.0,
            "Halstead difficulty is 99.0 (budget: 80.0)",
            "Simplify operators/operands in `untested_monster`: extract helpers, reduce distinct operators.",
        ),
        (
            "Statement Count",
            99.0,
            30.0,
            "Function has 99 statements (budget: 30)",
            "Split `untested_monster` into smaller focused functions.",
        ),
        (
            "ABC Score",
            99.0,
            10.0,
            "ABC score is 99.0 (budget: 10.0)",
            "Reduce assignments/branches/calls in `untested_monster` by extracting helpers.",
        ),
    ];

    assert_eq!(violations.len(), expected.len());
    for (violation, (metric, actual, limit, message, recommendation)) in
        violations.iter().zip(expected)
    {
        assert_eq!(violation.file, function.file);
        assert_eq!(violation.function_name, function.name);
        assert_eq!(violation.line_number, function.start_line);
        assert_eq!(violation.end_line, function.end_line);
        assert_eq!(violation.metric, metric);
        assert_eq!(violation.actual, actual);
        assert_eq!(violation.limit, limit);
        assert!(violation.breakdown.is_empty());
        assert_eq!(violation.message, message);
        assert_eq!(violation.recommendation, recommendation);
    }
}

const CURRENT_AST_REGRESSION_CASES: &[(&str, &str, &str, Option<&str>)] = &[
    (
        "src/regression.rs",
        r#"fn rust_case(value: i32) -> i32 {
            if value > 0 { value } else { 0 }
        }"#,
        "rust_case",
        Some("if_expression"),
    ),
    (
        "src/regression.ts",
        r#"function ts_case(items: number[]): number {
            for (const item of items) { if (item > 0) return item; }
            return 0;
        }"#,
        "ts_case",
        Some("for_in_statement"),
    ),
    (
        "src/regression.tsx",
        r#"function tsx_case(value: number) { return <span>{value}</span>; }"#,
        "tsx_case",
        None,
    ),
    (
        "src/regression.js",
        r#"function js_case(items) {
            using resource = acquire();
            for (const item of items) { if (item) return resource; }
            return null;
        }"#,
        "js_case",
        Some("for_in_statement"),
    ),
    (
        "src/regression.py",
        r#"def py_case(value):
            try:
                return value
            except* ValueError as error:
                return error
        "#,
        "py_case",
        Some("except_clause"),
    ),
    (
        "src/regression.go",
        r#"package main
        func go_case(value int) int {
            switch value {
            case 1:
                return 1
            default:
                return 0
            }
        }"#,
        "go_case",
        Some("expression_case"),
    ),
];

#[test]
fn test_current_tree_sitter_ast_regressions_across_dialects() {
    for &(path, code, expected_name, expected_branch) in CURRENT_AST_REGRESSION_CASES {
        let mut analyzer = ComplexityAnalyzer::new();
        let metrics = analyzer
            .analyze_file_checked(Path::new(path), code, Path::new("."))
            .unwrap_or_else(|error| panic!("failed to parse {path}: {error}"));
        assert_eq!(metrics.len(), 1, "expected one function in {path}");
        let function = &metrics[0];
        assert_eq!(function.name, expected_name, "wrong symbol in {path}");
        if let Some(expected_kind) = expected_branch {
            assert!(
                function
                    .cyclomatic_breakdown
                    .iter()
                    .any(|contribution| contribution.kind == expected_kind),
                "missing {expected_kind} branch in {path}: {:?}",
                function.cyclomatic_breakdown,
            );
        }
    }
}
