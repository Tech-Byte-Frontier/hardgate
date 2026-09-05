use hardgate::engines::ComplexityAnalyzer;
use std::path::Path;

#[test]
fn boolean_metrics_count_direct_grammar_operators_only() {
    let cases = [
        (
            "value.rs",
            "fn value(a: bool, b: bool) -> bool { (a && b) == true }",
            2,
        ),
        (
            "value.ts",
            "function value(a: boolean, b: boolean) { return (a && b) === true; }",
            2,
        ),
        (
            "value.tsx",
            "function value(a: boolean, b: boolean) { return (a || b) === false; }",
            2,
        ),
        (
            "value.js",
            "function value(a, b) { return (a && b) === true; }",
            2,
        ),
        (
            "value.go",
            "package value\nfunc value(a bool, b bool) bool { return (a && b) == true }",
            2,
        ),
        (
            "value.py",
            "def value(a, b):\n    return (a and b) == True\n",
            2,
        ),
        (
            "words.ts",
            "function words(and, or) { return and + or + '&&'; }",
            1,
        ),
        ("text.js", "function text(a) { return a + '||'; }", 1),
    ];
    let mut analyzer = ComplexityAnalyzer::new();
    for (path, text, expected) in cases {
        let functions = analyzer
            .analyze_file_checked(Path::new(path), text, Path::new("."))
            .unwrap();
        assert_eq!(functions.len(), 1, "{path}");
        assert_eq!(functions[0].cyclomatic, expected, "{path}");
        let booleans = functions[0]
            .cyclomatic_breakdown
            .iter()
            .filter(|item| item.kind == "boolean_operator")
            .count();
        assert_eq!(booleans as u32, expected - 1, "{path}");
    }
}

#[test]
fn parser_reuse_preserves_errors_language_switches_and_independent_trees() {
    let mut analyzer = ComplexityAnalyzer::new();
    let root = Path::new(".");
    for _ in 0..4 {
        let first = analyzer
            .analyze_file_checked(Path::new("first.rs"), "fn first() {}", root)
            .unwrap();
        assert!(
            analyzer
                .analyze_file_checked(Path::new("broken.rs"), "fn broken( {", root)
                .is_err()
        );
        let script = analyzer
            .analyze_file_checked(Path::new("script.js"), "function script() {}", root)
            .unwrap();
        let second = analyzer
            .analyze_file_checked(Path::new("SECOND.RS"), "fn second() {}", root)
            .unwrap();
        assert_eq!(first[0].name, "first");
        assert_eq!(script[0].name, "script");
        assert_eq!(second[0].name, "second");
    }
}
