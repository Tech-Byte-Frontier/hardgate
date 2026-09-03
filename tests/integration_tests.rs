use hardgate::config::CloneConfig;
use hardgate::config::CoverageConfig;
use hardgate::config::{AntiGamingConfig, InvariantRule};
use hardgate::engines::complexity::FunctionMetrics;
use hardgate::engines::coverage::FileCoverage;
use hardgate::engines::{
    AntiGamingScanner, CloneDetector, ComplexityAnalyzer, CoverageScorer, InvariantsChecker,
};
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
    let rules = vec![InvariantRule {
        name: Some("UI Boundary".to_string()),
        from: "src/components/**".to_string(),
        exclude: None,
        disallow_imports: Some(vec![
            "@tauri-apps/api*".to_string(),
            "src/db/**".to_string(),
        ]),
        disallow_calls: Some(vec!["fetch".to_string()]),
        disallow_tokens: Some(vec!["unsafe".to_string()]),
        message: Some("UI cannot talk to raw APIs".to_string()),
    }];

    let checker = InvariantsChecker::new(&rules);
    let root = Path::new(".");

    let offending_code = r#"
    import { invoke } from '@tauri-apps/api/core';
    fetch('https://api.example.com');
    "#;

    let violations =
        checker.check_file(Path::new("src/components/Header.tsx"), offending_code, root);

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
        file_cov
            .line_hits
            .insert(line, if line <= 2 { 1 } else { 0 });
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
        cognitive_breakdown: Vec::new(),
        cyclomatic_breakdown: Vec::new(),
    }];

    let violations = scorer.evaluate(&cov_map, &funcs, Path::new("."));
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "CRAP Score" && v.actual > 25.0)
    );
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

    let file_a = (
        PathBuf::from("src/a.rs"),
        format!("fn foo() {{\n{}\n}}", duplicate_body),
    );
    let file_b = (
        PathBuf::from("src/b.rs"),
        format!("fn bar() {{\n{}\n}}", duplicate_body),
    );

    let violations = detector.detect_clones(&[file_a, file_b], Path::new("."));
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].file_a, PathBuf::from("src/a.rs"));
    assert_eq!(violations[0].file_b, PathBuf::from("src/b.rs"));
}

#[test]
fn test_complexity_ast_breakdown() {
    let mut analyzer = ComplexityAnalyzer::new();
    let root = Path::new(".");

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

    let functions = analyzer.analyze_file(Path::new("src/test.rs"), code, root);
    assert_eq!(functions.len(), 1);
    let f = &functions[0];

    // Check that cognitive breakdown contains both the outer if and the nested if with increased score
    assert!(!f.cognitive_breakdown.is_empty());
    assert!(f.cognitive_breakdown.iter().any(|c| c.score >= 2)); // nested if has nesting depth >= 1 -> score >= 2
    assert!(!f.cyclomatic_breakdown.is_empty());

    // Check violations attach breakdown
    let budgets = hardgate::config::FunctionBudgets {
        max_cognitive: Some(1),
        ..Default::default()
    };
    let violations = ComplexityAnalyzer::check_violations(&functions, &budgets);
    assert_eq!(violations.len(), 1);
    assert!(!violations[0].breakdown.is_empty());
    assert!(violations[0].breakdown[0].score >= 2);
}

#[test]
fn test_orchestration_engine() {
    let config = hardgate::config::OrchestrationConfig {
        format_check: Some("echo formatting-checked".to_string()),
        format: Some("echo formatting-fixed".to_string()),
        lint: Some("echo linting-passed".to_string()),
        test_cmd: None,
    };

    let engine = hardgate::engines::OrchestrationEngine::new(&config);
    let root = Path::new(".");

    let fmt_check_res = engine.run_format_check(root).unwrap();
    assert!(fmt_check_res.is_ok());
    assert!(fmt_check_res.unwrap().output.contains("formatting-checked"));

    let fmt_res = engine.run_format(root).unwrap();
    assert!(fmt_res.is_ok());
    assert!(fmt_res.unwrap().output.contains("formatting-fixed"));

    let lint_res = engine.run_lint(root).unwrap();
    assert!(lint_res.is_ok());
    assert!(lint_res.unwrap().output.contains("linting-passed"));
}

#[test]
fn test_dead_code_analyzer() {
    let config = hardgate::config::DeadCodeConfig {
        enabled: true,
        entry_points: vec!["src/main.ts".to_string()],
        exclude: vec![],
    };

    let analyzer = hardgate::engines::DeadCodeAnalyzer::new(&config);
    let root = Path::new(".");

    let files = vec![
        PathBuf::from("src/main.ts"),
        PathBuf::from("src/used_service.ts"),
        PathBuf::from("src/dead_file.ts"),
    ];

    let contents = vec![
        (
            PathBuf::from("src/main.ts"),
            "import { usedFunc } from './used_service'; usedFunc();".to_string(),
        ),
        (
            PathBuf::from("src/used_service.ts"),
            "export function usedFunc() { return 42; }\nexport function unusedFunc() { return 0; }"
                .to_string(),
        ),
        (
            PathBuf::from("src/dead_file.ts"),
            "export const DEAD = 100;".to_string(),
        ),
    ];

    let violations = analyzer.analyze(&files, &contents, root);

    // Should catch src/dead_file.ts as unreferenced
    assert!(violations.iter().any(
        |v| v.file == Path::new("src/dead_file.ts") && v.violation_type == "Unreferenced File"
    ));
    // Should catch unusedFunc in used_service.ts as unused export
    assert!(
        violations
            .iter()
            .any(|v| v.symbol.as_deref() == Some("unusedFunc")
                && v.violation_type == "Unused Export")
    );
}

#[test]
fn test_ast_mutation_generator() {
    let mut generator = hardgate::engines::AstMutationGenerator::new();

    let code = r#"
    fn evaluate(a: i32, b: i32) -> bool {
        if a == b && a > 0 {
            return true;
        }
        false
    }
    "#;

    let mutants = generator.generate_mutants(Path::new("src/calc.rs"), code);
    assert!(!mutants.is_empty());

    // Should generate == -> !=
    assert!(
        mutants
            .iter()
            .any(|m| m.original == "==" && m.replacement == "!=")
    );
    // Should generate && -> ||
    assert!(
        mutants
            .iter()
            .any(|m| m.original == "&&" && m.replacement == "||")
    );
    // Should generate > -> <=
    assert!(
        mutants
            .iter()
            .any(|m| m.original == ">" && m.replacement == "<=")
    );
    // Should generate true -> false
    assert!(
        mutants
            .iter()
            .any(|m| m.original == "true" && m.replacement == "false")
    );
}

#[test]
fn test_clean_toml_formatting() {
    let toml_str = hardgate::config::HardgateConfig::generate_toml_template(
        hardgate::config::Preset::StrictAgent,
    );
    assert!(toml_str.contains("[gate]"));
    assert!(toml_str.contains("[orchestration]"));
    assert!(toml_str.contains("[analysis.dead_code]"));
    assert!(toml_str.contains("format_check = \"oxfmt --check .\""));

    // Check that it deserializes cleanly back into HardgateConfig
    let parsed: Result<hardgate::config::HardgateConfig, _> = toml::from_str(&toml_str);
    assert!(parsed.is_ok());
    let cfg = parsed.unwrap();
    assert_eq!(cfg.gate.preset, hardgate::config::Preset::StrictAgent);
    assert_eq!(
        cfg.orchestration.format_check.as_deref(),
        Some("oxfmt --check .")
    );
}

#[test]
fn test_clone_detector_excludes_advisory() {
    let config = CloneConfig {
        enabled: true,
        min_lines: 5,
        min_tokens: 25,
        excludes: Some(vec!["src/excluded/**".to_string()]),
    };

    let detector = CloneDetector::new(&config);
    let duplicate_body = r#"
        let mut sum = 0;
        for i in 0..100 {
            sum += i * 2;
            println!("Value: {}", sum);
        }
    "#;

    let file_a = (
        PathBuf::from("src/a.rs"),
        format!("fn foo() {{\n{}\n}}", duplicate_body),
    );
    let file_b = (
        PathBuf::from("src/excluded/b.rs"),
        format!("fn bar() {{\n{}\n}}", duplicate_body),
    );

    let files = vec![file_a, file_b];
    let excluded_count = detector.count_excluded_files(&files, Path::new("."));
    assert_eq!(excluded_count, 1);

    let excluded = detector.excluded_files(&files, Path::new("."));
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0], &PathBuf::from("src/excluded/b.rs"));

    // Since b.rs is excluded, clone detection produces no violations
    let violations = detector.detect_clones(&files, Path::new("."));
    assert_eq!(violations.len(), 0);
}

#[test]
fn test_discover_files_with_exclusions() {
    use hardgate::discovery::{DiscoverOptions, discover_files_with_exclusions};

    let result = discover_files_with_exclusions(DiscoverOptions {
        root: Path::new("."),
        diff_only: false,
        exclusions: &["tests/**".to_string()],
    })
    .expect("discovery should succeed");

    assert!(!result.files.is_empty());
    assert!(!result.excluded_files.is_empty());
    assert!(
        result
            .excluded_files
            .iter()
            .any(|f| f.ends_with("integration_tests.rs"))
    );
}

#[test]
fn test_gate_report_advisories_rendering() {
    use hardgate::GateReport;

    let mut report = GateReport::new("test-gate".to_string());
    report
        .advisories
        .push("25 files excluded from clone detection via hardgate.toml.".to_string());
    report
        .advisories
        .push("1 file excluded from file budget checks via hardgate.toml.".to_string());
    report.finalize(10, 50, 42);

    assert!(report.passed);

    let term = report.render_terminal();
    assert!(term.contains("25 files excluded from clone detection via hardgate.toml."));
    assert!(term.contains("1 file excluded from file budget checks via hardgate.toml."));
    assert!(term.contains("Advisory"));
    assert!(term.contains("PASS (All gates satisfied)"));

    let agent = report.render_agent();
    assert!(
        agent.contains(
            "> ⚠️ **Advisory**: 25 files excluded from clone detection via hardgate.toml."
        )
    );
    assert!(
        agent.contains(
            "> ⚠️ **Advisory**: 1 file excluded from file budget checks via hardgate.toml."
        )
    );
    assert!(agent.contains("✅ **Hardgate Passed**"));

    let json_str = report.render_json();
    assert!(json_str.contains("\"advisories\": ["));
    assert!(json_str.contains("25 files excluded from clone detection via hardgate.toml."));
}
