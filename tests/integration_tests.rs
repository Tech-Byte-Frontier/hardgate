use hardgate::config::{AntiGamingConfig, InvariantRule};
use hardgate::engines::{
    AntiGamingScanner, CloneDetector, ComplexityAnalyzer, CoverageScorer, InvariantsChecker,
};
use hardgate::config::CloneConfig;
use hardgate::config::CoverageConfig;
use hardgate::engines::complexity::FunctionMetrics;
use hardgate::engines::coverage::FileCoverage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[test]
fn test_anti_gaming_scanner() {
    let config = AntiGamingConfig::default();
    let scanner = AntiGamingScanner::new(&config);
    let root = Path::new(".");

    let ts_code = r#"
    // @ts-ignore
    const x: any = 42;
    /* eslint-disable */
    const y = 10;
    "#;
    let violations = scanner.scan_content(Path::new("src/test.ts"), ts_code, root);
    assert_eq!(violations.len(), 2);
    assert_eq!(violations[0].token, "@ts-ignore");
    assert_eq!(violations[1].token, "eslint-disable");

    let rust_code = r#"
    #[allow(unused_variables)]
    fn foo() {}
    "#;
    let violations = scanner.scan_content(Path::new("src/test.rs"), rust_code, root);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].token.contains("allow("));

    let py_code = r#"
    import sys  # type: ignore
    x = 1  # noqa
    "#;
    let violations = scanner.scan_content(Path::new("src/test.py"), py_code, root);
    assert_eq!(violations.len(), 2);
}

#[test]
fn test_complexity_analyzer_rust() {
    let mut analyzer = ComplexityAnalyzer::new();
    let root = Path::new(".");

    let code = r#"
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

    let functions = analyzer.analyze_file(Path::new("src/test.rs"), code, root);
    assert_eq!(functions.len(), 1);
    let f = &functions[0];
    assert_eq!(f.name, "complex_decision");
    assert_eq!(f.parameters, 2);
    assert!(f.cyclomatic >= 4);
    assert!(f.cognitive >= 3);
}

#[test]
fn test_complexity_analyzer_typescript() {
    let mut analyzer = ComplexityAnalyzer::new();
    let root = Path::new(".");

    let code = r#"
    function processItems(items: string[]): number {
        let count = 0;
        for (const item of items) {
            if (item.length > 5) {
                count++;
            }
        }
        return count;
    }
    "#;

    let functions = analyzer.analyze_file(Path::new("src/test.ts"), code, root);
    assert_eq!(functions.len(), 1);
    let f = &functions[0];
    assert_eq!(f.name, "processItems");
    assert_eq!(f.parameters, 1);
    assert!(f.cyclomatic >= 2);
    assert!(f.cognitive >= 2);
}

#[test]
fn test_complexity_analyzer_python() {
    let mut analyzer = ComplexityAnalyzer::new();
    let root = Path::new(".");

    let code = r#"
def calculate(data):
    total = 0
    for x in data:
        if x > 10:
            total += x
    return total
"#;

    let functions = analyzer.analyze_file(Path::new("src/test.py"), code, root);
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "calculate");
    assert_eq!(functions[0].parameters, 1);
    assert!(functions[0].cyclomatic >= 2);
}

#[test]
fn test_complexity_analyzer_go() {
    let mut analyzer = ComplexityAnalyzer::new();
    let root = Path::new(".");

    let code = r#"
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

    let functions = analyzer.analyze_file(Path::new("src/test.go"), code, root);
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "SumPositive");
    assert!(functions[0].cyclomatic >= 2);
}

#[test]
fn test_invariants_checker() {
    let rules = vec![
        InvariantRule {
            name: Some("UI Boundary".to_string()),
            from: "src/components/**".to_string(),
            exclude: None,
            disallow_imports: Some(vec!["@tauri-apps/api*".to_string(), "src/db/**".to_string()]),
            disallow_calls: Some(vec!["fetch".to_string()]),
            disallow_tokens: Some(vec!["unsafe".to_string()]),
            message: Some("UI cannot talk to raw APIs".to_string()),
        },
    ];

    let checker = InvariantsChecker::new(&rules);
    let root = Path::new(".");

    let offending_code = r#"
    import { invoke } from '@tauri-apps/api/core';
    fetch('https://api.example.com');
    "#;

    let violations = checker.check_file(
        Path::new("src/components/Header.tsx"),
        offending_code,
        root,
    );

    assert_eq!(violations.len(), 2);
    assert_eq!(violations[0].violation_type, "Disallowed Import");
    assert_eq!(violations[1].violation_type, "Disallowed Call");
}

#[test]
fn test_crap_score_calculation() {
    let config = CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(80.0),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: Some(25.0),
        critical_paths: None,
    };

    let scorer = CoverageScorer::new(&config);

    // High complexity (10) and low coverage (0.2)
    // CRAP = 10^2 * (1 - 0.2)^3 + 10 = 100 * (0.8)^3 + 10 = 100 * 0.512 + 10 = 61.2 -> fails gate
    let mut cov_map = HashMap::new();
    let mut file_cov = FileCoverage {
        file_path: PathBuf::from("src/calc.rs"),
        lines_found: 10,
        lines_hit: 2,
        ..Default::default()
    };
    for line in 1..=10 {
        file_cov.line_hits.insert(line, if line <= 2 { 1 } else { 0 });
    }
    cov_map.insert(file_cov.file_path.clone(), file_cov);

    let funcs = vec![FunctionMetrics {
        name: "untested_monster".to_string(),
        file: PathBuf::from("src/calc.rs"),
        start_line: 1,
        end_line: 10,
        lines: 10,
        parameters: 2,
        cyclomatic: 10,
        cognitive: 12,
        halstead_difficulty: 20.0,
        max_nesting_depth: 3,
        statements: 10,
    }];

    let violations = scorer.evaluate(&cov_map, &funcs, Path::new("."));
    assert!(violations.iter().any(|v| v.metric == "CRAP Score" && v.actual > 25.0));
}

#[test]
fn test_clone_detector() {
    let config = CloneConfig {
        enabled: true,
        min_lines: 5,
        min_tokens: 25,
        excludes: None,
    };

    let detector = CloneDetector::new(&config);
    let duplicate_body = r#"
        let mut sum = 0;
        for i in 0..100 {
            sum += i * 2;
            println!("Value: {}", sum);
        }
    "#;

    let file_a = (PathBuf::from("src/a.rs"), format!("fn foo() {{\n{}\n}}", duplicate_body));
    let file_b = (PathBuf::from("src/b.rs"), format!("fn bar() {{\n{}\n}}", duplicate_body));

    let violations = detector.detect_clones(&[file_a, file_b], Path::new("."));
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].file_a, PathBuf::from("src/a.rs"));
    assert_eq!(violations[0].file_b, PathBuf::from("src/b.rs"));
}
