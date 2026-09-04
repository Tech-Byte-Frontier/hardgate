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

#[test]
fn test_invariants_skip_nonmatching_and_excluded_paths() {
    let rules = vec![InvariantRule {
        name: Some("no-db".to_string()),
        from: "src/ui/**".to_string(),
        exclude: Some(vec!["src/ui/generated/**".to_string()]),
        disallow_imports: Some(vec!["db/**".to_string()]),
        disallow_calls: None,
        disallow_tokens: None,
        message: None,
    }];
    let checker = InvariantsChecker::new(&rules);
    let root = Path::new(".");
    let source = "use crate::db::pool;\n";

    assert!(
        checker
            .check_file(Path::new("src/api/handler.rs"), source, root)
            .is_empty(),
        "a path outside the rule boundary must not be scanned"
    );
    assert!(
        checker
            .check_file(Path::new("src/ui/generated/client.rs"), source, root)
            .is_empty(),
        "an excluded path must not be scanned"
    );
    assert_eq!(
        checker
            .check_file(Path::new("src/ui/visible.rs"), source, root)
            .len(),
        1
    );
}

#[test]
fn test_invariants_normalize_and_expand_rust_imports() {
    let rules = vec![InvariantRule {
        name: Some("no-db".to_string()),
        from: "src/**".to_string(),
        exclude: None,
        disallow_imports: Some(vec!["db/**".to_string()]),
        disallow_calls: None,
        disallow_tokens: None,
        message: None,
    }];
    let checker = InvariantsChecker::new(&rules);
    let violations = checker.check_file(
        Path::new("src/ui/data.rs"),
        concat!(
            "use crate::db::{pool, models};\n",
            "use self::db::query;\n",
            "use super::db::Connection;\n",
            "use crate::db::pool, crate::db::query;\n",
        ),
        Path::new("."),
    );

    assert_eq!(violations.len(), 4);
    assert_eq!(
        violations
            .iter()
            .map(|violation| violation.offending_target.as_str())
            .collect::<Vec<_>>(),
        vec![
            "crate::db::pool",
            "self::db::query",
            "super::db::Connection",
            "crate::db::pool",
        ]
    );
}
