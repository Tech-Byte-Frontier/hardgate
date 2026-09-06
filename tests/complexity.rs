#[path = "support/metrics.rs"]
mod metrics;

use hardgate::config::FunctionBudgets;
use hardgate::engines::complexity::FunctionMetrics;
use hardgate::engines::{ComplexityAnalyzer, ComplexityContribution};
use std::path::Path;

/// Analyze one fixture file and return its single function.
fn analyze_one(path: &str, code: &str) -> FunctionMetrics {
    let mut analyzer = ComplexityAnalyzer::new();
    let found = analyzer.analyze_file(Path::new(path), code, Path::new("."));
    assert_eq!(found.len(), 1);
    found.into_iter().next().unwrap()
}

/// OneAnalyzer per language, driven by a table so the supported cases share
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
    // (path, code, expected name, expected params, min cyclomatic)
    let cases = [
        ("src/test.rs", rust_code, "complex_decision", 2, 4),
        ("src/test.ts", ts_code, "classify", 1, 2),
    ];
    for (path, code, name, params, cyclo) in cases {
        let f = analyze_one(path, code);
        assert_eq!(f.name, name, "wrong symbol for {path}");
        assert_eq!(f.parameters, params, "wrong arity for {name}");
        assert!(f.cyclomatic >= cyclo, "low cyclomatic for {name}");
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

    // Both decision branches contribute to the cyclomatic score.
    assert!(!f.cyclomatic_breakdown.is_empty());

    let budgets = FunctionBudgets {
        max_cyclomatic: Some(1),
        ..Default::default()
    };
    let violations = ComplexityAnalyzer::check_violations(std::slice::from_ref(&f), &budgets);
    assert_eq!(violations.len(), 1);
    assert!(!violations[0].breakdown.is_empty());
    assert!(violations[0].breakdown[0].score == 1);
}

#[test]
fn test_statement_budget_enforced() {
    let budgets = FunctionBudgets {
        max_statements: Some(5),
        ..Default::default()
    };
    let bad = metrics::sample_metrics(99);
    let bad_violations = ComplexityAnalyzer::check_violations(&[bad], &budgets);
    let reported: Vec<&str> = bad_violations.iter().map(|v| v.metric.as_str()).collect();
    assert_eq!(reported, ["Statement Count"]);

    let good = metrics::sample_metrics(1);
    assert!(ComplexityAnalyzer::check_violations(&[good], &budgets).is_empty());
}

#[test]
fn test_limit_violation_details_and_order_are_preserved() {
    let mut function = metrics::sample_metrics(99);
    function.parameters = 6;
    function.max_nesting_depth = 6;
    let budgets = FunctionBudgets {
        max_parameters: Some(4),
        max_lines: Some(80),
        max_nesting_depth: Some(4),
        max_statements: Some(30),
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
            "Review code and documentation in `untested_monster` separately; extract cohesive code only where it improves clarity.",
        ),
        (
            "Nesting Depth",
            6.0,
            4.0,
            "Max nesting depth is 6 (budget: 4)",
            "Use early returns or guard clauses to reduce nesting depth in `untested_monster`.",
        ),
        (
            "Statement Count",
            99.0,
            30.0,
            "Function has 99 statements (budget: 30)",
            "Split `untested_monster` into smaller focused functions.",
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

#[test]
fn test_equal_score_breakdown_uses_line_tie_breaker() {
    let mut function = metrics::sample_metrics(4);
    function.cyclomatic_breakdown = [41, 7, 29, 13, 5, 19]
        .into_iter()
        .map(|line| ComplexityContribution {
            line,
            column: 1,
            kind: "if_statement".to_string(),
            description: "conditional branch (`if`)".to_string(),
            score: 1,
        })
        .collect();

    let budgets = FunctionBudgets {
        max_cyclomatic: Some(1),
        ..Default::default()
    };
    let violations = ComplexityAnalyzer::check_violations(&[function], &budgets);
    assert_eq!(violations.len(), 1);
    let lines: Vec<usize> = violations[0]
        .breakdown
        .iter()
        .map(|contribution| contribution.line)
        .collect();
    assert_eq!(lines, [5, 7, 13, 19, 29]);
}

#[test]
fn test_function_expressions_cover_language_name_fallbacks() {
    const EMPTY_EXPRESSION: &str = "const typed = function () {};";
    let cases = [
        ("src/expression.ts", "typed"),
        ("src/expression.tsx", "typed"),
        ("src/expression.js", "typed"),
    ];
    for (path, expected_name) in cases {
        let mut analyzer = ComplexityAnalyzer::new();
        let metrics = analyzer
            .analyze_file_checked(Path::new(path), EMPTY_EXPRESSION, Path::new("."))
            .unwrap_or_else(|error| panic!("failed to parse {path}: {error}"));
        assert_eq!(metrics.len(), 1, "expected one function in {path}");
        assert_eq!(metrics[0].name, expected_name);
    }

    let mut analyzer = ComplexityAnalyzer::new();
    let anonymous_source = format!("{EMPTY_EXPRESSION}\n(function () {{}});");
    let metrics = analyzer
        .analyze_file_checked(
            Path::new("src/anonymous.js"),
            &anonymous_source,
            Path::new("."),
        )
        .expect("anonymous function expressions should parse");
    assert_eq!(metrics.len(), 2);
    assert!(metrics.iter().any(|function| function.name == "typed"));
    assert!(metrics.iter().any(|function| function.name == "anonymous"));
}

#[test]
fn test_property_and_field_identifiers_are_analyzed_across_dialects() {
    let cases = [(
        "src/property.js",
        "function read(record) { return record.value; }",
        "read",
    )];

    for (path, source, expected_name) in cases {
        let mut analyzer = ComplexityAnalyzer::new();
        let metrics = analyzer
            .analyze_file_checked(Path::new(path), source, Path::new("."))
            .unwrap_or_else(|error| panic!("failed to parse {path}: {error}"));
        assert_eq!(metrics.len(), 1, "expected one function in {path}");
        assert_eq!(metrics[0].name, expected_name);
    }
}

#[test]
fn test_unchecked_parse_failure_and_nonmatching_root_are_safe() {
    let mut analyzer = ComplexityAnalyzer::new();
    let metrics = analyzer
        .analyze_file_checked(Path::new("outside.rs"), "fn outside() {}", Path::new("src"))
        .expect("valid source outside the root should still parse");
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].file, Path::new("outside.rs"));

    assert!(
        analyzer
            .analyze_file(Path::new("broken.rs"), "fn broken( {", Path::new("src"))
            .is_empty()
    );
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
