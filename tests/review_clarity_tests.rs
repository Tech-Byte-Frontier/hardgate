use hardgate::GateReport;
use hardgate::commands::run_static_gate_snapshot;
use hardgate::config::{HardgateConfig, Preset};
use std::path::PathBuf;

fn analyze(path: &str, text: &str, config: &HardgateConfig) -> GateReport {
    let (mut report, files, _, functions) =
        run_static_gate_snapshot(config, &[(PathBuf::from(path), text.into())]).unwrap();
    report.functions = functions;
    report.finalize(files.len(), report.functions.len(), 0);
    report
}

#[test]
fn rust_javascript_and_typescript_show_documentation_without_changing_physical_budgets() {
    let mut config = Preset::Balanced.to_default_config();
    config.roles.source.max_lines = Some(3);
    for (path, source, docs, physical) in [
        (
            "src/lib.rs",
            "//! Guide\n/// Details\npub fn answer() -> &'static str {\n    // Explanation\n    \"/// text, not a comment\"\n}\n\n",
            2,
            7,
        ),
        (
            "src/index.js",
            "/** Guide\n * Details\n */\nexport function answer() {\n    // Explanation\n    return \"/* text */\";\n}\n\n",
            3,
            8,
        ),
        (
            "src/index.ts",
            "/** Guide\n * Details\n */\nexport const answer = (): string => {\n    // Explanation\n    return \"/* text */\";\n};\n\n",
            3,
            8,
        ),
    ] {
        let report = analyze(path, source, &config);
        let size = &report.file_sizes[0].size;
        assert_eq!(size.physical_lines, physical, "{path}: {size:?}");
        assert_eq!(
            (
                size.code_lines,
                size.documentation_lines,
                size.comment_lines,
                size.blank_lines
            ),
            (3, docs, 1, 1),
            "{path}: {size:?}"
        );
        assert_eq!(report.budget_violations[0].actual, physical);
        assert_eq!(report.budget_violations[0].limit, 3);
        assert!(!report.passed);
        for display in [
            report.render_terminal(),
            report.render_agent(),
            report.render_compact(),
        ] {
            assert!(
                display.contains(&format!("{docs} documentation")),
                "{display}"
            );
            assert!(display.contains("3 code"), "{display}");
        }
        assert_eq!(report.functions[0].size.as_ref().unwrap().comment_lines, 1);
    }
}

#[test]
fn eight_metric_findings_are_presented_as_two_function_review_targets() {
    let mut config = Preset::Balanced.to_default_config();
    config.budgets.functions.max_lines = Some(1);
    config.budgets.functions.max_parameters = Some(1);
    config.budgets.functions.max_nesting_depth = Some(1);
    config.budgets.functions.max_cyclomatic = Some(1);
    config.budgets.functions.max_statements = None;
    let source = "fn first(a: bool, b: bool) -> bool {\n    if a {\n        if b { return true; }\n    }\n    false\n}\nfn second(a: bool, b: bool) -> bool {\n    if a {\n        if b { return true; }\n    }\n    false\n}\n";
    let mut report = analyze("src/lib.rs", source, &config);
    assert_eq!(report.total_violations(), 8, "{report:?}");
    assert_eq!(report.function_reviews().len(), 2);
    assert!(
        report
            .function_reviews()
            .iter()
            .all(|group| group.metrics.len() == 4)
    );
    assert_eq!(report.summary().function_review_targets, 2);
    assert_eq!(
        report.render_terminal().matches("  --> src/lib.rs").count(),
        2
    );
    assert_eq!(
        report
            .render_agent()
            .matches("Review: simplify the shared control flow")
            .count(),
        2
    );
    assert_eq!(
        report.render_compact().matches("error[complexity]").count(),
        2
    );
    let full: serde_json::Value = serde_json::from_str(&report.render_json().unwrap()).unwrap();
    assert_eq!(full["review_targets"].as_array().unwrap().len(), 2);
    report.display.max_diagnostics = Some(3);
    let limited: serde_json::Value = serde_json::from_str(&report.render_json().unwrap()).unwrap();
    assert_eq!(limited["summary"]["total_errors"], 8);
    assert_eq!(limited["summary"]["function_review_targets"], 2);
    assert_eq!(limited["shown"], 3);
    assert_eq!(limited["omitted"], 5);
    assert_eq!(limited["review_targets"].as_array().unwrap().len(), 1);
    assert_eq!(
        limited["review_targets"][0]["metrics"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn anonymous_functions_on_the_same_line_keep_distinct_review_targets() {
    let mut config = Preset::Balanced.to_default_config();
    config.budgets.functions.max_parameters = Some(1);
    let report = analyze(
        "src/index.js",
        "consume((a,b) => a+b, (c,d) => c+d);\n",
        &config,
    );
    let groups = report.function_reviews();
    assert_eq!(groups.len(), 2, "{report:?}");
    assert_eq!(groups[0].line, groups[1].line);
    assert_eq!(groups[0].function_name, groups[1].function_name);
    assert_ne!(groups[0].column, groups[1].column);
}
