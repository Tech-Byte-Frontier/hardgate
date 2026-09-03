use hardgate::config::AntiGamingConfig;
use hardgate::engines::AntiGamingScanner;
use std::path::Path;

// Fixture pragmas are assembled with `concat!` so this file never contains
// a live suppression literal that the scanner itself would flag.
const TS_PRAGMA: &str = concat!("// @ts-", "ignore");
const ESLINT_PRAGMA: &str = concat!("/* eslint-", "disable */");
const RUST_ATTR: &str = concat!("#[allow", "(unused_variables)]");
const PY_TYPE_IGNORE: &str = concat!("# type:", " ignore");
const PY_NOQA: &str = concat!("#", " noqa");

#[test]
fn test_anti_gaming_scanner() {
    let scanner = AntiGamingScanner::new(&AntiGamingConfig::default());
    let root = Path::new(".");

    let ts_code = format!("{TS_PRAGMA}\nconst x: any = 42;\n{ESLINT_PRAGMA}\nconst y = 10;\n");
    let violations = scanner.scan_content(Path::new("src/test.ts"), &ts_code, root);
    assert_eq!(violations.len(), 2);
    assert_eq!(violations[0].token, concat!("@ts-", "ignore"));
    assert_eq!(violations[1].token, concat!("eslint-", "disable"));

    let rust_code = format!("{RUST_ATTR}\nfn foo() {{}}\n");
    let violations = scanner.scan_content(Path::new("src/test.rs"), &rust_code, root);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].token.contains("allow("));

    let py_code = format!("import sys  {PY_TYPE_IGNORE}\nx = 1  {PY_NOQA}\n");
    let violations = scanner.scan_content(Path::new("src/test.py"), &py_code, root);
    assert_eq!(violations.len(), 2);
}

#[test]
fn test_anti_gaming_extended_tokens() {
    let scanner = AntiGamingScanner::new(&AntiGamingConfig::default());
    let root = Path::new(".");
    let cases = vec![
        ("a.ts", concat!("// @ts-", "expect-error\nconst x = 1;\n")),
        (
            "b.ts",
            concat!("// biome-", "ignore lint: reason\nconst x = 1;\n"),
        ),
        ("c.py", concat!("x = 1  # ruff:", " noqa: F401\n")),
        (
            "d.rs",
            concat!("#[cfg(test)] #[allow", "(dead_code)]\nfn f() {}\n"),
        ),
    ];
    for (file, code) in cases {
        let found = scanner.scan_content(Path::new(file), code, root);
        assert!(!found.is_empty(), "expected violation in {file}");
    }
    // A pragma-looking string literal is data, not a suppression.
    let data = concat!("const s = \"@ts-", "ignore is just data\";\n");
    assert!(
        scanner
            .scan_content(Path::new("a.ts"), data, root)
            .is_empty()
    );
}
