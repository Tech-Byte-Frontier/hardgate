#[path = "support/fs.rs"]
mod fs;
#[path = "support/trees.rs"]
mod trees;

use fs::tempdir;
use hardgate::config::{ExclusionConfig, FileBudgets};
use hardgate::discovery::{DiscoverOptions, DiscoveryResult, discover_files_with_exclusions};
use hardgate::engines::check_file_budgets;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use trees::{has_suffix, write_tree};

#[test]
fn test_clean_toml_formatting() {
    let toml_str = hardgate::config::HardgateConfig::generate_toml_template(
        hardgate::config::Preset::StrictAgent,
    );
    assert!(toml_str.contains("[gate]"));
    assert!(toml_str.contains("[orchestration]"));
    assert!(toml_str.contains("[analysis.dead_code]"));
    assert!(toml_str.contains("format_check = \"oxfmt --check .\""));

    // The template must deserialize cleanly back into a config.
    let parsed: Result<hardgate::config::HardgateConfig, _> = toml::from_str(&toml_str);
    assert!(parsed.is_ok());
    let cfg = parsed.unwrap();
    assert_eq!(cfg.gate.preset, hardgate::config::Preset::StrictAgent);
    assert_eq!(
        cfg.orchestration.format_check.as_deref(),
        Some("oxfmt --check .")
    );
}

#[test]
fn test_config_merge_preserves_user_sections() {
    use hardgate::config::HardgateConfig;

    let tmp = tempdir("cfg");
    let cfg_path = tmp.join("hardgate.toml");
    std::fs::write(
        &cfg_path,
        r#"
[gate]
name = "merge-test"
preset = "balanced"
strict = false

[budgets.functions]
max_cyclomatic = 42

[coverage]
enabled = false
report = "coverage/lcov.info"
min_line_percent = 77.0

[mutation]
enabled = false
min_score = 70.0
timeout_secs = 5
max_mutants = 7

[orchestration]
format_check = "my-fmt --check"
test_cmd = "my-test --all"

[clones]
enabled = false
min_lines = 9
min_tokens = 99
excludes = ["gen/**"]

[anti_gaming]
disallow_suppressions = false

[invariants]
enforce = false
"#,
    )
    .unwrap();

    let cfg = HardgateConfig::load_or_default(Some(&cfg_path)).unwrap();
    assert_eq!(cfg.gate.name, "merge-test");
    assert_eq!(cfg.budgets.functions.max_cyclomatic, Some(42));
    // Untouched keys keep the balanced preset scaling.
    assert_eq!(cfg.budgets.functions.max_cognitive, Some(22));
    // Explicit sections win wholesale, even `enabled = false`.
    assert!(!cfg.coverage.enabled);
    assert_eq!(cfg.coverage.min_line_percent, Some(77.0));
    assert!(!cfg.mutation.enabled);
    assert_eq!(cfg.mutation.timeout_secs, Some(5));
    assert_eq!(cfg.mutation.max_mutants, Some(7));
    assert_eq!(
        cfg.orchestration.format_check.as_deref(),
        Some("my-fmt --check")
    );
    assert_eq!(cfg.orchestration.test_cmd.as_deref(), Some("my-test --all"));
    assert!(!cfg.clones.enabled);
    assert_eq!(cfg.clones.min_lines, 9);
    assert!(!cfg.anti_gaming.disallow_suppressions);
    assert!(!cfg.invariants.enforce);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_budget_exclusions_glob() {
    let tmp = tempdir("bud");
    std::fs::create_dir_all(tmp.join("src/generated")).unwrap();
    let gen_file = tmp.join("src/generated/big.rs");
    // Ten lines against a budget of five: violates unless excluded.
    std::fs::write(&gen_file, "a\n".repeat(10)).unwrap();
    let keep_file = tmp.join("src/keep.rs");
    std::fs::write(&keep_file, "a\n".repeat(10)).unwrap();

    let budgets = FileBudgets {
        max_bytes: None,
        max_lines: HashMap::from([("rs".to_string(), 5), ("default".to_string(), 5)]),
        exclusions: ExclusionConfig {
            paths: vec!["src/generated/**".to_string()],
        },
    };

    let gen_violations = check_file_budgets(&gen_file, &budgets, &tmp);
    assert!(
        gen_violations.is_empty(),
        "glob exclusion should suppress violations, got {gen_violations:?}"
    );
    let keep_violations = check_file_budgets(&keep_file, &budgets, &tmp);
    assert_eq!(keep_violations.len(), 1);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_discover_files_with_exclusions() {
    // Hermetic: build a temp tree instead of depending on the repo CWD.
    let tmp = tempdir("disc");
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::create_dir_all(tmp.join("tests")).unwrap();
    std::fs::write(tmp.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(tmp.join("tests/sample_test.rs"), "// fixture\n").unwrap();

    let result = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: false,
        exclusions: &["tests/**".to_string()],
    })
    .expect("discovery should succeed");

    assert!(result.files.iter().any(|f| f.ends_with("src/main.rs")));
    assert!(
        result
            .excluded_files
            .iter()
            .any(|f| f.ends_with("sample_test.rs"))
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_shell_words_split_quotes() {
    use hardgate::engines::orchestration::shell_words_split;

    assert_eq!(
        shell_words_split("cargo test -- --exact foo"),
        vec!["cargo", "test", "--", "--exact", "foo"]
    );
    assert_eq!(
        shell_words_split("cargo test -- \"my test name\""),
        vec!["cargo", "test", "--", "my test name"]
    );
    assert_eq!(
        shell_words_split("pnpm test 'a b' c"),
        vec!["pnpm", "test", "a b", "c"]
    );
    assert_eq!(
        shell_words_split("cmd \"a b c\" --path '/tmp/my dir/x'"),
        vec!["cmd", "a b c", "--path", "/tmp/my dir/x"]
    );
}

#[test]
fn test_orchestration_engine() {
    let config = hardgate::config::OrchestrationConfig {
        format_check: Some("echo formatting-checked".to_string()),
        format: Some("echo formatting-fixed".to_string()),
        lint: Some("echo linting-passed".to_string()),
        test_cmd: None,
    };

    let engine = hardgate::engines::OrchestrationEngine::new(&config);
    let root = Path::new(".");

    let check = engine.run_format_check(root).unwrap();
    assert!(check.is_ok());
    assert!(check.unwrap().output.contains("formatting-checked"));

    let fmt = engine.run_format(root).unwrap();
    assert!(fmt.is_ok());
    assert!(fmt.unwrap().output.contains("formatting-fixed"));

    let lint = engine.run_lint(root).unwrap();
    assert!(lint.is_ok());
    assert!(lint.unwrap().output.contains("linting-passed"));
}

#[test]
fn test_gate_report_advisories_rendering() {
    use hardgate::GateReport;

    let mut report = GateReport::new("test-gate".to_string());
    report
        .advisories
        .push("25 files excluded from clone detection via hardgate.toml.".to_string());
    report
        .advisories
        .push("1 file excluded from file budget checks via hardgate.toml.".to_string());
    report.finalize(10, 50, 42);

    assert!(report.passed);

    let term = report.render_terminal();
    assert!(term.contains("25 files excluded from clone detection via hardgate.toml."));
    assert!(term.contains("1 file excluded from file budget checks via hardgate.toml."));
    assert!(term.contains("warning:"));
    assert!(term.contains("result:"));
    assert!(term.contains("pass"));

    let agent = report.render_agent();
    assert!(
        agent.contains(
            "> ⚠️ **Advisory**: 25 files excluded from clone detection via hardgate.toml."
        )
    );
    assert!(
        agent.contains(
            "> ⚠️ **Advisory**: 1 file excluded from file budget checks via hardgate.toml."
        )
    );
    assert!(agent.contains("✅ **Hardgate Passed**"));

    let json_str = report.render_json();
    assert!(json_str.contains("\"advisories\": ["));
    assert!(json_str.contains("25 files excluded from clone detection via hardgate.toml."));
}

fn discover_all(root: &Path) -> DiscoveryResult {
    let no_exclusions: &[String] = &[];
    discover_files_with_exclusions(DiscoverOptions {
        root,
        diff_only: false,
        exclusions: no_exclusions,
    })
    .expect("discovery should succeed")
}

#[test]
fn test_discovery_applies_standard_filters() {
    let tmp = tempdir("disc-standard");
    write_tree(&tmp, &["keep.rs", ".hidden.rs", "skip.gen.rs", "Makefile"]);
    std::fs::write(tmp.join(".ignore"), "*.gen.rs\n").unwrap();

    let result = discover_all(&tmp);
    assert!(has_suffix(&result.files, "keep.rs"));
    // Hidden files, .ignore rules, and extensionless files must all be out.
    for gone in [".hidden.rs", "skip.gen.rs", "Makefile"] {
        assert!(
            !has_suffix(&result.files, gone),
            "{gone} should be filtered"
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_discovery_reads_parent_ignore_files() {
    let tmp = tempdir("disc-parents");
    write_tree(&tmp, &["inner/keep.rs", "inner/skipme.rs"]);
    std::fs::write(tmp.join(".ignore"), "skipme.rs\n").unwrap();

    let files = discover_all(&tmp.join("inner")).files;
    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("keep.rs"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_discovery_respects_git_ignore_and_exclude() {
    let tmp = tempdir("disc-git");
    // A bare `.git` dir marks a repo root for the walker; no git binary needed.
    write_tree(
        &tmp,
        &["keep.rs", "skip.gen.rs", "skip.exc.rs", ".git/info/exclude"],
    );
    std::fs::write(tmp.join(".gitignore"), "*.gen.rs\n").unwrap();
    std::fs::write(tmp.join(".git/info/exclude"), "*.exc.rs\n").unwrap();

    let found = discover_all(&tmp).files;
    assert_eq!(found.len(), 1, "only keep.rs should survive, got {found:?}");
    assert!(found[0].ends_with("keep.rs"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_discovery_respects_global_git_excludes() {
    let tmp = tempdir("disc-global");
    write_tree(&tmp, &["keep.rs", "skip.hgglobal.rs"]);
    std::fs::create_dir_all(tmp.join(".git")).unwrap();
    let excludes = tmp.join("global-excludes");
    std::fs::write(&excludes, "*.hgglobal.rs\n").unwrap();
    let gitconfig = tmp.join("test.gitconfig");
    std::fs::write(
        &gitconfig,
        format!("[core]\n\texcludesFile = {}\n", excludes.display()),
    )
    .unwrap();

    // Hermetic: GIT_CONFIG_GLOBAL replaces $HOME/.gitconfig for this lookup.
    // The pattern is unique to this fixture, so parallel tests are unaffected.
    // SAFETY: edition 2024 marks env mutation unsafe; the var is removed
    // immediately after discovery and matches no other test's files.
    let key = "GIT_CONFIG_GLOBAL";
    unsafe { std::env::set_var(key, &gitconfig) };
    let result = discover_all(&tmp);
    unsafe { std::env::remove_var(key) };

    assert!(has_suffix(&result.files, "keep.rs"));
    assert!(!has_suffix(&result.files, "skip.hgglobal.rs"));
    let _ = std::fs::remove_dir_all(&tmp);
}

fn git_in(repo: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .expect("git binary must be available for diff tests")
        .success();
    assert!(ok, "git {args:?} failed");
}

#[test]
fn test_diff_discovery_tracks_modified_and_untracked() {
    let tmp = tempdir("disc-diff");
    write_tree(&tmp, &["src/keep.rs", "gen/skip.rs"]);
    git_in(&tmp, &["init", "-q"]);
    git_in(&tmp, &["config", "user.email", "t@t.t"]);
    git_in(&tmp, &["config", "user.name", "t"]);
    git_in(&tmp, &["config", "commit.gpgsign", "false"]);
    git_in(&tmp, &["add", "-A"]);
    git_in(&tmp, &["commit", "-qm", "base"]);

    std::fs::write(tmp.join("src/keep.rs"), "// changed\n").unwrap();
    std::fs::write(tmp.join("gen/skip.rs"), "// changed\n").unwrap();
    write_tree(&tmp, &["new.rs", "weird.rs/inner.rs"]);

    let diffed = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: true,
        exclusions: &["gen/**".to_string()],
    })
    .expect("diff discovery should succeed");

    assert!(has_suffix(&diffed.files, "src/keep.rs"));
    assert!(has_suffix(&diffed.files, "new.rs"));
    assert!(
        diffed.files.iter().all(|f| f.is_file()),
        "directories must never be listed, got {:?}",
        diffed.files
    );
    assert!(has_suffix(&diffed.excluded_files, "gen/skip.rs"));
    let _ = std::fs::remove_dir_all(&tmp);
}
