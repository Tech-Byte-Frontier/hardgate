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
        abc_score: 12.0,
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

    // Hermetic: build a temp tree instead of depending on the repo CWD.
    let tmp = std::env::temp_dir().join(format!("hardgate-test-disc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::create_dir_all(tmp.join("tests")).unwrap();
    std::fs::write(tmp.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(tmp.join("tests/integration_tests.rs"), "// fixture\n").unwrap();

    let result = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: false,
        exclusions: &["tests/**".to_string()],
    })
    .expect("discovery should succeed");

    assert!(result.files.iter().any(|f| f.ends_with("src/main.rs")));
    assert!(
        result
            .excluded_files
            .iter()
            .any(|f| f.ends_with("integration_tests.rs"))
    );
    let _ = std::fs::remove_dir_all(&tmp);
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

#[test]
fn test_budget_exclusions_glob() {
    use hardgate::config::{ExclusionConfig, FileBudgets};
    use hardgate::engines::check_file_budgets;
    use std::collections::HashMap;

    let tmp = std::env::temp_dir().join(format!("hardgate-test-bud-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src/generated")).unwrap();
    let gen_file = tmp.join("src/generated/big.rs");
    // 10 lines, budget is 5 -> would violate without exclusion.
    std::fs::write(&gen_file, "a\n".repeat(10)).unwrap();
    let keep_file = tmp.join("src/keep.rs");
    std::fs::write(&keep_file, "a\n".repeat(10)).unwrap();

    let budgets = FileBudgets {
        max_bytes: None,
        max_lines: HashMap::from([("rs".to_string(), 5), ("default".to_string(), 5)]),
        exclusions: ExclusionConfig {
            paths: vec!["src/generated/**".to_string()],
        },
    };

    let gen_violations = check_file_budgets(&gen_file, &budgets, &tmp);
    assert!(
        gen_violations.is_empty(),
        "glob exclusion should suppress violations, got {:?}",
        gen_violations
    );
    let keep_violations = check_file_budgets(&keep_file, &budgets, &tmp);
    assert_eq!(keep_violations.len(), 1);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_complexity_advanced_budgets_enforced() {
    use hardgate::config::FunctionBudgets;
    use hardgate::engines::complexity::FunctionMetrics;

    let mk = |halstead: f64, statements: usize, abc: f64| FunctionMetrics {
        name: "f".to_string(),
        file: PathBuf::from("src/a.rs"),
        start_line: 1,
        end_line: 10,
        lines: 10,
        parameters: 1,
        cyclomatic: 2,
        cognitive: 2,
        halstead_difficulty: halstead,
        max_nesting_depth: 1,
        statements,
        abc_score: abc,
        cognitive_breakdown: Vec::new(),
        cyclomatic_breakdown: Vec::new(),
    };

    let budgets = FunctionBudgets {
        max_halstead_difficulty: Some(10.0),
        max_statements: Some(5),
        max_abc: Some(10.0),
        ..Default::default()
    };
    let violations = ComplexityAnalyzer::check_violations(&[mk(99.0, 99, 99.0)], &budgets);
    assert!(violations.iter().any(|v| v.metric == "Halstead Difficulty"));
    assert!(violations.iter().any(|v| v.metric == "Statement Count"));
    assert!(violations.iter().any(|v| v.metric == "ABC Score"));

    let ok = ComplexityAnalyzer::check_violations(&[mk(1.0, 1, 1.0)], &budgets);
    assert!(ok.is_empty());
}

#[test]
fn test_config_merge_preserves_user_sections() {
    use hardgate::config::HardgateConfig;

    let tmp = std::env::temp_dir().join(format!("hardgate-test-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg_path = tmp.join("hardgate.toml");
    std::fs::write(
        &cfg_path,
        r#"
[gate]
name = "merge-test"
preset = "balanced"
strict = false

[budgets.functions]
max_cyclomatic = 42

[coverage]
enabled = false
report = "coverage/lcov.info"
min_line_percent = 77.0

[mutation]
enabled = false
min_score = 70.0
timeout_secs = 5
max_mutants = 7

[orchestration]
format_check = "my-fmt --check"
test_cmd = "my-test --all"

[clones]
enabled = false
min_lines = 9
min_tokens = 99
excludes = ["gen/**"]

[anti_gaming]
disallow_suppressions = false

[invariants]
enforce = false
"#,
    )
    .unwrap();

    let cfg = HardgateConfig::load_or_default(Some(&cfg_path)).unwrap();
    assert_eq!(cfg.gate.name, "merge-test");
    assert_eq!(cfg.budgets.functions.max_cyclomatic, Some(42));
    // Preset scaling for balanced must survive for untouched keys.
    assert_eq!(cfg.budgets.functions.max_cognitive, Some(22));
    // User sections must win wholesale, including explicit `enabled = false`.
    assert!(!cfg.coverage.enabled);
    assert_eq!(cfg.coverage.min_line_percent, Some(77.0));
    assert!(!cfg.mutation.enabled);
    assert_eq!(cfg.mutation.timeout_secs, Some(5));
    assert_eq!(cfg.mutation.max_mutants, Some(7));
    assert_eq!(
        cfg.orchestration.format_check.as_deref(),
        Some("my-fmt --check")
    );
    assert_eq!(cfg.orchestration.test_cmd.as_deref(), Some("my-test --all"));
    assert!(!cfg.clones.enabled);
    assert_eq!(cfg.clones.min_lines, 9);
    assert!(!cfg.anti_gaming.disallow_suppressions);
    assert!(!cfg.invariants.enforce);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_shell_words_split_quotes() {
    use hardgate::engines::orchestration::shell_words_split;

    assert_eq!(
        shell_words_split("cargo test -- --exact foo"),
        vec!["cargo", "test", "--", "--exact", "foo"]
    );
    assert_eq!(
        shell_words_split("cargo test -- \"my test name\""),
        vec!["cargo", "test", "--", "my test name"]
    );
    assert_eq!(
        shell_words_split("pnpm test 'a b' c"),
        vec!["pnpm", "test", "a b", "c"]
    );
    assert_eq!(
        shell_words_split("cmd \"a b c\" --path '/tmp/my dir/x'"),
        vec!["cmd", "a b c", "--path", "/tmp/my dir/x"]
    );
}

#[test]
fn test_lcov_checksum_and_paths() {
    use hardgate::config::CoverageConfig;
    use hardgate::engines::CoverageScorer;

    let tmp = std::env::temp_dir().join(format!("hardgate-test-lcov-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let report = tmp.join("lcov.info");
    std::fs::write(
        &report,
        "SF:/repo/src/calc.rs\nDA:1,0,AAAAAAAA\nDA:2,0,BBBBBBBB\nDA:3,1\nLF:3\nLH:1\nend_of_record\n",
    )
    .unwrap();

    let scorer = CoverageScorer::new(&CoverageConfig {
        enabled: true,
        report: None,
        min_line_percent: Some(90.0),
        min_function_percent: None,
        min_branch_percent: None,
        max_crap_score: Some(25.0),
        critical_paths: Some(vec!["src/calc.rs".to_string()]),
    });
    let map = scorer.parse_lcov(&report).unwrap();
    // Checksum suffix must not drop the line.
    let cov = map.values().next().unwrap();
    assert_eq!(cov.line_hits.get(&1), Some(&0));
    assert_eq!(cov.line_hits.get(&2), Some(&0));

    // Absolute report path must match relative function path.
    let funcs = vec![FunctionMetrics {
        name: "big".to_string(),
        file: PathBuf::from("src/calc.rs"),
        start_line: 1,
        end_line: 3,
        lines: 3,
        parameters: 0,
        cyclomatic: 10,
        cognitive: 10,
        halstead_difficulty: 5.0,
        max_nesting_depth: 1,
        statements: 3,
        abc_score: 5.0,
        cognitive_breakdown: Vec::new(),
        cyclomatic_breakdown: Vec::new(),
    }];
    let violations = scorer.evaluate(&map, &funcs, Path::new("/repo"));
    assert!(violations.iter().any(|v| v.metric == "CRAP Score"));
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Critical Path 100% Coverage")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_mutation_report_parsers() {
    use hardgate::config::MutationConfig;
    use hardgate::engines::MutationGatekeeper;

    let tmp = std::env::temp_dir().join(format!("hardgate-test-mut-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let gatekeeper = MutationGatekeeper::new(&MutationConfig {
        enabled: true,
        min_score: Some(85.0),
        reject_timeouts: false,
        reports: None,
        test_cmd: None,
        timeout_secs: Some(10),
        max_mutants: Some(30),
    });

    // Stryker format: 1 killed / 1 survived = 50% < 85% -> violation.
    let stryker = tmp.join("stryker.json");
    std::fs::write(
        &stryker,
        r#"{"files": {"a.rs": {"mutants": [{"status": "Killed"}, {"status": "Survived"}]}}}"#,
    )
    .unwrap();
    let v = gatekeeper.evaluate_report(&stryker).unwrap();
    assert!(v.iter().any(|x| x.metric == "Mutation Kill Rate"));

    // cargo-mutants format: all caught = 100% -> no violation.
    let cm = tmp.join("cm.json");
    std::fs::write(
        &cm,
        r#"{"outcomes": [{"summary": "caught"}, {"summary": "caught"}]}"#,
    )
    .unwrap();
    let v2 = gatekeeper.evaluate_report(&cm).unwrap();
    assert!(v2.is_empty());

    // Generic format.
    let generic = tmp.join("gen.json");
    std::fs::write(&generic, r#"{"killed": 9, "survived": 1, "timeout": 0}"#).unwrap();
    let v3 = gatekeeper.evaluate_report(&generic).unwrap();
    assert!(v3.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_anti_gaming_extended_tokens() {
    use hardgate::config::AntiGamingConfig;
    use hardgate::engines::AntiGamingScanner;

    let scanner = AntiGamingScanner::new(&AntiGamingConfig::default());
    let root = Path::new(".");
    let cases = vec![
        ("a.ts", "// @ts-expect-error\nconst x = 1;\n"),
        ("b.ts", "// biome-ignore lint: reason\nconst x = 1;\n"),
        ("c.py", "x = 1  # ruff: noqa: F401\n"),
        ("d.rs", "#[cfg(test)] #[allow(dead_code)]\nfn f() {}\n"),
    ];
    for (file, code) in cases {
        let v = scanner.scan_content(Path::new(file), code, root);
        assert!(!v.is_empty(), "expected violation in {}", file);
    }
    // String literal must not flag.
    let v = scanner.scan_content(
        Path::new("a.ts"),
        "const s = \"@ts-ignore is just data\";\n",
        root,
    );
    assert!(v.is_empty());
}

#[test]
fn test_invariants_rust_use_and_comments() {
    use hardgate::config::InvariantRule;
    use hardgate::engines::InvariantsChecker;

    let rules = vec![InvariantRule {
        name: Some("no-db".to_string()),
        from: "src/ui/**".to_string(),
        exclude: None,
        disallow_imports: Some(vec!["*db*".to_string()]),
        disallow_calls: None,
        disallow_tokens: None,
        message: None,
    }];
    let checker = InvariantsChecker::new(&rules);
    let root = Path::new(".");

    let v = checker.check_file(
        Path::new("src/ui/a.rs"),
        "use crate::db::pool;\nfn f() {}\n",
        root,
    );
    assert_eq!(v.len(), 1);

    // Inline comment must not flag.
    let v2 = checker.check_file(
        Path::new("src/ui/a.rs"),
        "// use crate::db::pool;\nfn f() {}\n",
        root,
    );
    assert!(v2.is_empty());

    // String literal must not flag.
    let v3 = checker.check_file(
        Path::new("src/ui/a.rs"),
        "const S: &str = \"use crate::db::pool\";\nfn f() {}\n",
        root,
    );
    assert!(v3.is_empty());
}

#[test]
fn test_dead_code_word_boundary() {
    let config = hardgate::config::DeadCodeConfig {
        enabled: true,
        entry_points: vec!["src/main.ts".to_string()],
        exclude: vec![],
    };
    let analyzer = hardgate::engines::DeadCodeAnalyzer::new(&config);
    let root = Path::new(".");

    let files = vec![PathBuf::from("src/main.ts"), PathBuf::from("src/svc.ts")];
    let contents = vec![
        (
            PathBuf::from("src/main.ts"),
            "import { used } from './svc'; used();".to_string(),
        ),
        (
            PathBuf::from("src/svc.ts"),
            "export function used() { return 1; }\nexport function unusedFunc() { return 0; }"
                .to_string(),
        ),
    ];
    let violations = analyzer.analyze(&files, &contents, root);
    // `used` in main.ts must not suppress `unusedFunc` via substring.
    assert!(
        violations
            .iter()
            .any(|v| v.symbol.as_deref() == Some("unusedFunc"))
    );
    assert!(
        !violations
            .iter()
            .any(|v| v.symbol.as_deref() == Some("used"))
    );
}

#[test]
fn test_clone_actual_tokens_not_threshold() {
    let config = CloneConfig {
        enabled: true,
        min_lines: 5,
        min_tokens: 25,
        excludes: None,
    };
    let detector = CloneDetector::new(&config);
    let body = "let mut sum = 0;\n".repeat(12);
    let file_a = (
        PathBuf::from("src/a.rs"),
        format!("fn foo() {{\n{}\n}}", body),
    );
    let file_b = (
        PathBuf::from("src/b.rs"),
        format!("fn bar() {{\n{}\n}}", body),
    );
    let violations = detector.detect_clones(&[file_a, file_b], Path::new("."));
    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].tokens >= 25,
        "tokens should be actual (>= min), got {}",
        violations[0].tokens
    );
}
