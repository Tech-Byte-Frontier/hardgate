use hardgate::config::InvariantRule;
use hardgate::engines::InvariantsChecker;
use std::path::Path;

fn ui_rules() -> Vec<InvariantRule> {
    vec![InvariantRule {
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
    }]
}

#[test]
fn test_invariants_checker() {
    let checker = InvariantsChecker::new(&ui_rules());
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
fn test_invariants_rust_use_and_comments() {
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

    let flagged = checker.check_file(
        Path::new("src/ui/a.rs"),
        "use crate::db::pool;\nfn f() {}\n",
        root,
    );
    assert_eq!(flagged.len(), 1);

    // A commented-out import is not an import.
    let commented = checker.check_file(
        Path::new("src/ui/a.rs"),
        "// use crate::db::pool;\nfn f() {}\n",
        root,
    );
    assert!(commented.is_empty());

    // Neither is one inside a string literal.
    let quoted = checker.check_file(
        Path::new("src/ui/a.rs"),
        "const S: &str = \"use crate::db::pool\";\nfn f() {}\n",
        root,
    );
    assert!(quoted.is_empty());
}
