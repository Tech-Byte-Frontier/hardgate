use hardgate::engines::AntiGamingScanner;
use hardgate::engines::ComplexityAnalyzer;
use hardgate::engines::complexity::SupportedLanguage;
use std::path::Path;

fn analyze(source: &str) -> Vec<hardgate::engines::FunctionMetrics> {
    ComplexityAnalyzer::new()
        .analyze_file_checked(Path::new("src/worker.py"), source, Path::new("."))
        .unwrap()
}

#[test]
fn python_functions_parameters_branches_and_nested_scopes() {
    let source = "def choose(a, /, b=1, *args, flag=True, **kwargs):\n    if a and flag:\n        return b\n    elif b:\n        return a\n    else:\n        return 0\n\nasync def worker(items):\n    def nested(x):\n        return 1 if x else 0\n    return [nested(x) for x in items if x]\n";
    let functions = analyze(source);
    assert_eq!(functions.len(), 3);
    let choose = &functions[0];
    assert_eq!(choose.name, "choose");
    assert_eq!(choose.parameters, 5);
    assert_eq!(choose.cyclomatic, 4);
    assert_eq!(choose.max_nesting_depth, 1);
    assert_eq!(functions[1].name, "worker");
    assert_eq!(
        functions[1].cyclomatic, 3,
        "nested function decisions do not belong to worker"
    );
    assert_eq!(functions[2].cyclomatic, 2);
}

#[test]
fn decorated_methods_lambdas_exceptions_and_loops_are_measured() {
    let source = "class Worker:\n    @staticmethod\n    def run(items):\n        try:\n            for item in items:\n                while item:\n                    item -= 1\n        except ValueError:\n            raise\n        return lambda x, y=1: x or y\n";
    let functions = analyze(source);
    assert_eq!(functions.len(), 2);
    assert_eq!(functions[0].name, "run");
    assert_eq!(functions[0].cyclomatic, 4);
    assert_eq!(functions[0].max_nesting_depth, 2);
    assert_eq!(functions[1].name, "lambda");
    assert_eq!(functions[1].parameters, 2);
    assert_eq!(functions[1].cyclomatic, 2);
}

#[test]
fn python_syntax_failure_stays_incomplete_and_go_remains_unsupported() {
    assert!(SupportedLanguage::parse_file_checked(Path::new("bad.py"), "def bad(:\n").is_err());
    assert!(SupportedLanguage::parse_file_checked(Path::new("main.go"), "package main\n").is_err());
    let classified = hardgate::discovery::ClassifiedFile::new(Path::new("render.py"));
    assert!(classified.ast_supported);
    assert_eq!(classified.role, hardgate::discovery::FileRole::Source);
}

#[test]
fn python_suppressions_are_blocking_tokens_but_strings_are_data() {
    let scanner = AntiGamingScanner::new(&Default::default());
    let content = "x = 1 # noqa: F841\ny = 2 # type: ignore\nz = 3 # pragma: no cover\n# pylint: disable=all\nlabel = 'noqa'\n";
    assert_eq!(
        scanner
            .scan_content(Path::new("worker.py"), content, Path::new("."))
            .len(),
        4
    );
}

#[test]
fn python_import_rules_cover_aliases_relative_and_multiline_imports() {
    use hardgate::engines::InvariantsChecker;
    let config: hardgate::config::HardgateConfig = toml::from_str(
        r#"
[gate]
preset='custom'
[[invariants.rules]]
name='boundary'
from='**/*.py'
disallow_imports=['private.*', '.private', '.secret', 'blocked']
message='Private dependency'
"#,
    )
    .unwrap();
    let checker = InvariantsChecker::new(&config.invariants.rules);
    let content = "import public, private.module as alias\nfrom private import (\n    service as renamed,\n)\nfrom . import private, secret\nfrom blocked import *\ntext = \"import private.data\"\n# import private.comment\n";
    let violations = checker.check_file(Path::new("src/service.py"), content, Path::new("."));
    let mut targets: Vec<_> = violations
        .iter()
        .map(|v| v.offending_target.as_str())
        .collect();
    targets.sort();
    assert_eq!(
        targets,
        vec![
            ".private",
            ".secret",
            "blocked",
            "private.module",
            "private.service"
        ]
    );
    assert!(violations.iter().all(|v| v.line_number <= 6));
    let unrestricted = InvariantsChecker::new(&[]);
    assert!(
        unrestricted
            .check_file(Path::new("other.py"), content, Path::new("."))
            .is_empty()
    );
}
