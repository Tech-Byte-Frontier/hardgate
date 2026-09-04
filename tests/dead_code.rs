#[path = "support/deadcode.rs"]
mod deadcode;

use deadcode::dead_code_analyzer;
use std::path::{Path, PathBuf};

/// Build entry + contents, run the analyzer, return violations.
fn find_dead_code(
    files: Vec<PathBuf>,
    contents: Vec<(PathBuf, String)>,
) -> Vec<hardgate::engines::DeadCodeViolation> {
    let analyzer = dead_code_analyzer(vec!["src/main.ts".to_string()]);
    analyzer.analyze(&files, &contents, Path::new("."))
}

fn entry(path: &str, code: &str) -> (PathBuf, String) {
    (PathBuf::from(path), code.to_string())
}

#[test]
fn test_dead_code_analyzer() {
    let files = vec![
        PathBuf::from("src/main.ts"),
        PathBuf::from("src/used_service.ts"),
        PathBuf::from("src/dead_file.ts"),
    ];

    let contents = vec![
        entry(
            "src/main.ts",
            "import { usedFunc } from './used_service'; usedFunc();",
        ),
        entry(
            "src/used_service.ts",
            "export function usedFunc() { return 42; }\nexport function unusedFunc() { return 0; }",
        ),
        entry("src/dead_file.ts", "export const DEAD = 100;"),
    ];

    let violations = find_dead_code(files, contents);

    // An orphaned module left behind by a rewritten plan must be reported.
    assert!(violations.iter().any(
        |v| v.file == Path::new("src/dead_file.ts") && v.violation_type == "Unreferenced File"
    ));
    // As must a superseded export inside a live module.
    assert!(
        violations
            .iter()
            .any(|v| v.symbol.as_deref() == Some("unusedFunc")
                && v.violation_type == "Unused Export")
    );
}

#[test]
fn test_dead_code_word_boundary() {
    let files = vec![PathBuf::from("src/main.ts"), PathBuf::from("src/svc.ts")];
    let contents = vec![
        entry("src/main.ts", "import { used } from './svc'; used();"),
        entry(
            "src/svc.ts",
            "export function used() { return 1; }\nexport function unusedFunc() { return 0; }",
        ),
    ];
    let violations = find_dead_code(files, contents);
    // `used` must not rescue `unusedFunc` through substring matching.
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
fn cargo_build_scripts_and_path_modules_are_reachable() {
    let files = vec![
        PathBuf::from("build.rs"),
        PathBuf::from("crates/worker/build.rs"),
        PathBuf::from("src/main.rs"),
        PathBuf::from("src/engines/mutation/js.rs"),
        PathBuf::from("src/engines/mutation/js_resolver_tests.rs"),
        PathBuf::from("src/engines/mutation/runner.rs"),
        PathBuf::from("src/engines/mutation/runner_tests.rs"),
        PathBuf::from("src/orphan.rs"),
    ];
    let contents = vec![
        entry("build.rs", "fn main() {}"),
        entry("crates/worker/build.rs", "fn main() {}"),
        entry("src/main.rs", "mod js; mod runner;"),
        entry(
            "src/engines/mutation/js.rs",
            "#[cfg(test)] #[path = \"js_resolver_tests.rs\"] mod tests;",
        ),
        entry(
            "src/engines/mutation/js_resolver_tests.rs",
            "fn resolver_tests() {}",
        ),
        entry(
            "src/engines/mutation/runner.rs",
            "#[cfg(test)] #[path = \"runner_tests.rs\"] mod tests;",
        ),
        entry(
            "src/engines/mutation/runner_tests.rs",
            "fn runner_tests() {}",
        ),
        entry("src/orphan.rs", "fn orphan() {}"),
    ];

    let violations = find_dead_code(files, contents);
    let unreferenced_files = violations
        .iter()
        .filter(|violation| violation.violation_type == "Unreferenced File")
        .map(|violation| violation.file.clone())
        .collect::<Vec<_>>();

    assert_eq!(unreferenced_files, vec![PathBuf::from("src/orphan.rs")]);
}
