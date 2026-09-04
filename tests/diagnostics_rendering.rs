use hardgate::GateReport;
use hardgate::engines::{
    BudgetViolation, CloneViolation, ComplexityContribution, ComplexityViolation,
    CoverageViolation, DeadCodeViolation, InvariantViolation, MutationViolation,
    OrchestrationViolation, SuppressionViolation,
};
use std::path::PathBuf;

fn contribution(line: usize, score: u32, description: &str) -> ComplexityContribution {
    ComplexityContribution {
        line,
        column: 3,
        kind: "branch".to_string(),
        description: description.to_string(),
        score,
    }
}

fn every_category_report() -> GateReport {
    let mut report = GateReport::new("diagnostics".to_string());
    report
        .advisories
        .push("advisory evidence is visible to every human renderer".to_string());
    report.suppression_violations.push(SuppressionViolation {
        file: PathBuf::from("src/main.rs"),
        line_number: 4,
        token: "#[allow(dead_code)]".to_string(),
        line_content: "#[allow(dead_code)]".to_string(),
        message: "suppression is prohibited".to_string(),
    });
    report.complexity_violations.push(ComplexityViolation {
        file: PathBuf::from("src/flow.rs"),
        function_name: "route_request".to_string(),
        line_number: 10,
        end_line: 28,
        metric: "Cognitive Complexity".to_string(),
        actual: 18.0,
        limit: 10.0,
        breakdown: vec![contribution(14, 7, "nested branch")],
        message: "complexity exceeds the configured budget".to_string(),
        recommendation: "Extract the nested branch.".to_string(),
    });
    // The empty breakdown exercises the no-contributors path alongside the
    // populated breakdown above.
    report.complexity_violations.push(ComplexityViolation {
        file: PathBuf::from("src/flow.rs"),
        function_name: "parse_request".to_string(),
        line_number: 32,
        end_line: 42,
        metric: "Cyclomatic Complexity".to_string(),
        actual: 12.0,
        limit: 8.0,
        breakdown: Vec::new(),
        message: "complexity exceeds the configured budget".to_string(),
        recommendation: "Split the parser into helpers.".to_string(),
    });
    report.budget_violations.push(BudgetViolation {
        file: PathBuf::from("src/large.rs"),
        metric: "Physical Lines (.rs)".to_string(),
        actual: 600,
        limit: 400,
        message: "file exceeds its physical budget".to_string(),
    });
    report.invariant_violations.push(InvariantViolation {
        file: PathBuf::from("src/ui/view.tsx"),
        line_number: 8,
        rule_name: "ui-no-db".to_string(),
        violation_type: "import".to_string(),
        offending_target: "db/client".to_string(),
        line_content: "import { client } from 'db/client';".to_string(),
        message: "UI code cannot import the database layer".to_string(),
    });
    report.clone_violations.push(CloneViolation {
        file_a: PathBuf::from("src/a.rs"),
        lines_a: (2, 9),
        file_b: PathBuf::from("src/b.rs"),
        lines_b: (20, 27),
        tokens: 72,
        lines: 8,
        fingerprint: "diagnostics-clone".to_string(),
        message: "duplicated token window".to_string(),
        recommendation: "Extract the shared helper.".to_string(),
    });
    report.coverage_violations.push(CoverageViolation {
        file: PathBuf::from("src/flow.rs"),
        function_name: Some("route_request".to_string()),
        metric: "Line Coverage".to_string(),
        actual: 71.2,
        limit: 95.0,
        message: "uncovered lines remain".to_string(),
        recommendation: "Add tests for every route.".to_string(),
    });
    report.coverage_violations.push(CoverageViolation {
        file: PathBuf::from("src/flow.rs"),
        function_name: None,
        metric: "Global Branch Coverage".to_string(),
        actual: 42.0,
        limit: 90.0,
        message: "branch floor is not met".to_string(),
        recommendation: "Exercise both sides of each branch.".to_string(),
    });
    report.mutation_violations.push(MutationViolation {
        report_file: PathBuf::from("target/mutants.json"),
        metric: "Mutation Score".to_string(),
        actual: 61.5,
        limit: 85.0,
        message: "surviving mutants remain".to_string(),
        recommendation: "Add assertions that kill the survivors.".to_string(),
    });
    report.dead_code_violations.push(DeadCodeViolation {
        file: PathBuf::from("src/orphan.ts"),
        line_number: Some(3),
        symbol: Some("orphan".to_string()),
        violation_type: "Unused Export".to_string(),
        message: "export is never referenced".to_string(),
        recommendation: "Remove the export or add a consumer.".to_string(),
    });
    report.dead_code_violations.push(DeadCodeViolation {
        file: PathBuf::from("src/unused.ts"),
        line_number: None,
        symbol: None,
        violation_type: "Unreferenced File".to_string(),
        message: "file is not part of the active graph".to_string(),
        recommendation: "Delete the file or import it.".to_string(),
    });
    report
        .orchestration_violations
        .push(OrchestrationViolation {
            step: "lint".to_string(),
            command: "cargo clippy --all-targets".to_string(),
            exit_code: Some(1),
            output: "clippy found an error".to_string(),
            recommendation: "Resolve the lint diagnostics.".to_string(),
        });
    report
        .orchestration_violations
        .push(OrchestrationViolation {
            step: "test".to_string(),
            command: "cargo test".to_string(),
            exit_code: None,
            output: "process terminated without an exit code".to_string(),
            recommendation: "Rerun the test command and inspect its output.".to_string(),
        });
    report.finalize(7, 13, 17);
    report
}

fn assert_contains_all(output: &str, needles: &[&str]) {
    for needle in needles {
        assert!(output.contains(needle), "missing `{needle}` in:\n{output}");
    }
}

#[test]
fn empty_report_uses_pass_paths_for_each_renderer() {
    let mut report = GateReport::new("empty".to_string());
    report.finalize(0, 0, 1);

    let agent = report.render_agent();
    let terminal = report.render_terminal();
    let compact = report.render_compact();

    assert_contains_all(
        &agent,
        &["✅ **Hardgate Passed**", "0 files", "0 functions"],
    );
    assert_contains_all(&terminal, &["hardgate [empty]", "pass", "result: pass"]);
    assert_contains_all(&compact, &["hardgate [empty]", "pass", "result: pass"]);
    assert!(!agent.contains("### "));
    assert!(!terminal.contains("error["));
    assert!(!compact.contains("error["));
}

#[test]
fn every_category_is_rendered_with_actionable_details() {
    let report = every_category_report();
    assert!(!report.passed);
    assert_eq!(report.total_violations(), 13);

    let agent = report.render_agent();
    assert_contains_all(
        &agent,
        &[
            "❌ **Hardgate Failed**",
            "> ⚠️ **Advisory**",
            "### 🚫 Anti-Gaming",
            "### ⚡ Complexity",
            "### 📦 Physical Budget",
            "### 🏛️ Architecture",
            "### 👥 Duplication Clone",
            "### 🎯 Coverage / CRAP",
            "### 🧬 Mutation Floor",
            "### 🍂 Dead Code",
            "### 🛠️ Tool Failure",
            "Key AST Contributors:",
            "Line 14: +7 for nested branch",
            "route_request",
            "Global Branch Coverage",
            "target/mutants.json",
        ],
    );
    assert!(!agent.contains("Key AST Contributors:\n- Actionable"));

    let terminal = report.render_terminal();
    assert_contains_all(
        &terminal,
        &[
            "warning:",
            "error[anti-gaming]",
            "error[complexity]",
            "error[file-budget]",
            "error[architecture]",
            "error[clone]",
            "error[coverage]",
            "error[mutation]",
            "error[dead-code]",
            "error[tool]",
            "key contributors:",
            "L14: +7 for nested branch",
            "src/unused.ts",
            "summary: 7 files, 13 functions in 17ms",
            "result: fail (13 errors)",
        ],
    );

    let compact = report.render_compact();
    assert_contains_all(
        &compact,
        &[
            "warning:",
            "error[anti-gaming]",
            "error[complexity]",
            "error[file-budget]",
            "error[architecture]",
            "error[clone]",
            "error[coverage]",
            "error[mutation]",
            "error[dead-code]",
            "error[tool]",
            "src/unused.ts",
            "result: fail (13 errors)",
        ],
    );
    assert!(!compact.contains("help:"));
    assert!(!compact.contains("key contributors"));
}
