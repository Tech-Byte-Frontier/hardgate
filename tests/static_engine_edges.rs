#[path = "support/fs.rs"]
mod fs;
#[path = "common/fs_git.rs"]
mod fs_git;
#[path = "support/trees.rs"]
mod trees;

use fs_git::{commit_baseline, init_repo, write};
use hardgate::config::AntiGamingConfig;
use hardgate::discovery::{DiscoverOptions, discover_files_with_exclusions, filter_files_by_paths};
use hardgate::engines::{AntiGamingScanner, ComplexityAnalyzer};
use std::path::{Path, PathBuf};
use trees::{has_suffix, write_tree};

#[test]
fn complexity_checked_paths_cover_unsupported_syntax_and_arrows() {
    let mut analyzer = ComplexityAnalyzer;
    assert!(
        analyzer
            .analyze_file_checked(Path::new("src/style.css"), "body {}", Path::new("."))
            .expect("unsupported files should be skipped")
            .is_empty()
    );

    let error = analyzer
        .analyze_file_checked(Path::new("src/broken.rs"), "fn broken( {", Path::new("."))
        .expect_err("syntax errors must be reported by the checked API");
    assert!(error.to_string().contains("syntax errors"));

    let source = r#"
const compute = (value) => value ? value : 0;
((x) => x + 1);
function outer(value) {
    const inner = () => value;
    return inner();
}
"#;
    let metrics = analyzer
        .analyze_file_checked(Path::new("src/arrows.js"), source, Path::new("."))
        .expect("valid JavaScript should parse");
    let names: Vec<&str> = metrics.iter().map(|metric| metric.name.as_str()).collect();
    assert!(names.contains(&"compute"));
    assert!(names.contains(&"anonymous"));
    assert!(names.contains(&"outer"));
    assert!(names.contains(&"inner"));
    let compute = metrics
        .iter()
        .find(|metric| metric.name == "compute")
        .expect("the named arrow should be analyzed");
    assert!(compute.cyclomatic_breakdown.iter().any(|contribution| {
        contribution.kind == "ternary_expression"
            && contribution.description == "ternary operator (`? :`)"
    }));
}

#[test]
fn anti_gaming_distinguishes_comments_strings_and_rust_attribute_boundaries() {
    const TS_IGNORE: &str = concat!("@ts-", "ignore");
    const TS_EXPECT: &str = concat!("@ts-", "expect-error");
    const RUST_ALLOW: &str = concat!("#[allow", "(dead_code)]");
    const CUSTOM: &str = concat!("FORBID_", "TOKEN");
    let scanner = AntiGamingScanner::new(&AntiGamingConfig {
        disallow_suppressions: true,
        custom_forbidden_tokens: vec![CUSTOM.to_string()],
    });
    let source = [
        format!("const text = \"{TS_IGNORE}\";"),
        format!("const code = {TS_IGNORE};"),
        format!("// {TS_IGNORE}"),
        format!(" * {TS_EXPECT}"),
        RUST_ALLOW.replace("#[allow", "#![allow"),
        format!("foo; {RUST_ALLOW}"),
        format!("fn body() {{}} {RUST_ALLOW}"),
        format!("mod scope {{ {RUST_ALLOW}"),
        format!("// {CUSTOM}"),
    ]
    .join("\n");

    let violations = scanner.scan_content(Path::new("src/edge.rs"), &source, Path::new("."));
    assert_eq!(violations.len(), 7);
    assert!(
        violations
            .iter()
            .any(|violation| violation.token == TS_IGNORE)
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.token == TS_EXPECT)
    );
    assert!(violations.iter().any(
        |violation| violation.token == CUSTOM && violation.message.contains("forbidden token")
    ));
    assert!(
        violations
            .iter()
            .filter(|violation| violation.token.contains("allow("))
            .count()
            >= 4
    );
}

#[test]
fn discovery_reports_invalid_filters_and_git_errors() {
    let tmp = fs::tempdir("static-discovery-edges");
    write_tree(&tmp, &["src/lib.rs", "src/app.ts"]);
    let discovered = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: false,
        exclusions: &[],
    })
    .expect("filesystem discovery should succeed");
    assert!(has_suffix(&discovered.files, "src/lib.rs"));

    let unchanged = filter_files_by_paths(discovered.files.clone(), &[], &tmp).unwrap();
    assert_eq!(unchanged, discovered.files);
    let src_files =
        filter_files_by_paths(discovered.files.clone(), &[PathBuf::from("src")], &tmp).unwrap();
    assert_eq!(src_files.len(), 2);
    let dot_files = filter_files_by_paths(
        vec![PathBuf::from("src/lib.rs")],
        &[PathBuf::from(".")],
        &tmp,
    )
    .unwrap();
    assert_eq!(dot_files, vec![PathBuf::from("src/lib.rs")]);

    let missing = filter_files_by_paths(
        discovered.files.clone(),
        &[PathBuf::from("missing.ts")],
        &tmp,
    )
    .expect_err("missing explicit scopes must fail loudly");
    assert!(missing.to_string().contains("Path not found"));

    let bad_glob = ["[".to_string()];
    let glob_error = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: false,
        exclusions: &bad_glob,
    })
    .expect_err("invalid exclusion globs must be reported");
    assert!(glob_error.to_string().contains("Invalid exclusion glob"));

    let git_error = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: true,
        exclusions: &[],
    })
    .expect_err("diff discovery outside git must fail");
    assert!(git_error.to_string().contains("git status"));
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn diff_discovery_skips_changed_dependencies_and_non_inventory_files() {
    let tmp = fs::tempdir("static-discovery-diff");
    write(&tmp, "src/lib.rs", "fn baseline() {}\n");
    write(&tmp, "node_modules/pkg/index.js", "const baseline = 1;\n");
    write(&tmp, "README.md", "baseline\n");
    init_repo(&tmp);
    commit_baseline(&tmp, "baseline");

    write(&tmp, "src/lib.rs", "fn changed() {}\n");
    write(&tmp, "node_modules/pkg/index.js", "const changed = 1;\n");
    write(&tmp, "README.md", "changed\n");
    let diffed = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: true,
        exclusions: &[],
    })
    .expect("git diff discovery should succeed");

    assert!(has_suffix(&diffed.files, "src/lib.rs"));
    assert!(!has_suffix(&diffed.files, "node_modules/pkg/index.js"));
    assert!(!has_suffix(&diffed.files, "README.md"));
    let _ = std::fs::remove_dir_all(tmp);
}
