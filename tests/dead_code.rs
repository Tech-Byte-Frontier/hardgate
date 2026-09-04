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

fn fixture_paths(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

fn unreferenced_files(violations: &[hardgate::engines::DeadCodeViolation]) -> Vec<PathBuf> {
    violations
        .iter()
        .filter(|violation| violation.violation_type == "Unreferenced File")
        .map(|violation| violation.file.clone())
        .collect()
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
    let unreferenced_files = unreferenced_files(&violations);

    assert_eq!(unreferenced_files, vec![PathBuf::from("src/orphan.rs")]);
}

#[test]
fn custom_globs_preserve_generated_entries_and_vendor_exclusions() {
    let analyzer = hardgate::engines::DeadCodeAnalyzer::new(&hardgate::config::DeadCodeConfig {
        enabled: true,
        entry_points: vec!["src/generated/**".to_string()],
        exclude: vec!["src/vendor/**".to_string()],
    });
    assert!(analyzer.is_enabled());

    let files = vec![
        PathBuf::from("src/main.ts"),
        PathBuf::from("src/generated/entry.ts"),
        PathBuf::from("src/vendor/legacy.ts"),
        PathBuf::from("src/feature.ts"),
    ];
    let contents = files
        .iter()
        .map(|path| (path.clone(), String::new()))
        .collect::<Vec<_>>();

    let violations = analyzer.analyze(&files, &contents, Path::new("."));
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].file, Path::new("src/feature.ts"));
    assert_eq!(violations[0].violation_type, "Unreferenced File");
}

#[test]
fn import_and_rust_graph_edges_keep_reachable_exports() {
    let files = fixture_paths(&[
        "src/main.ts",
        "src/lib.rs",
        "src/feature.ts",
        "src/component.tsx",
        "src/plugin.js",
        "src/rust_mod.rs",
        "src/helper.rs",
        "src/shared.rs",
        "src/nested.rs",
        "src/orphan.rs",
    ]);
    let contents = vec![
        entry(
            "src/main.ts",
            "import { feature } from './feature.ts';\nimport Component from './component.tsx';\nconst plugin = require('./plugin.js');\nfeature(); void Component; void plugin;",
        ),
        entry("src/lib.rs", "mod rust_mod;"),
        entry(
            "src/feature.ts",
            "export function feature() { return 1; }\nexport function _private() { return 0; }\nexport const stale = 2;",
        ),
        entry(
            "src/component.tsx",
            "export default function Component() {}",
        ),
        entry("src/plugin.js", "module.exports = {};"),
        entry(
            "src/rust_mod.rs",
            "mod helper;\nuse crate::shared;\nuse super::shared;\n#[path = \"nested.rs\"] mod nested;",
        ),
        entry("src/helper.rs", "pub fn helper() {}"),
        entry("src/shared.rs", "pub fn shared() {}"),
        entry("src/nested.rs", "pub fn nested() {}"),
        entry("src/orphan.rs", "fn orphan() {}"),
    ];

    let violations = find_dead_code(files, contents);
    let unreferenced_files = unreferenced_files(&violations);
    assert_eq!(unreferenced_files, vec![PathBuf::from("src/orphan.rs")]);

    let stale = violations
        .iter()
        .find(|violation| violation.symbol.as_deref() == Some("stale"))
        .expect("unreferenced exports should be reported");
    assert_eq!(stale.file, Path::new("src/feature.ts"));
    assert_eq!(stale.line_number, Some(3));
    assert!(
        !violations
            .iter()
            .any(|violation| violation.symbol.as_deref() == Some("_private"))
    );
}
