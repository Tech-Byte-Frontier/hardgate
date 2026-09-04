#[path = "support/fs.rs"]
mod fs;

use fs::tempdir;
use hardgate::discovery::{ClassifiedFile, FileRole};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BASE_CONFIG: &str = r#"
[gate]
name = "p0-fixture"
preset = "custom"
strict = true
enforce_classified_sources = true

[budgets.files]
max_bytes = 100000

[budgets.files.max_lines]
default = 10000
rs = 10000

[budgets.functions]
max_cyclomatic = 100
max_cognitive = 100
max_parameters = 20
max_lines = 1000
max_nesting_depth = 20

[anti_gaming]
disallow_suppressions = true

[clones]
enabled = true
min_lines = 3
min_tokens = 10

[coverage]
enabled = false

[mutation]
enabled = false
"#;

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    fn new(prefix: &str) -> Self {
        Self(tempdir(prefix))
    }
}

impl Deref for FixtureRoot {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn hardgate(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("hardgate binary should run")
}

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(target, content).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn init_repo(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "hardgate@example.invalid"]);
    git(root, &["config", "user.name", "Hardgate Test"]);
    git(root, &["config", "commit.gpgsign", "false"]);
}

fn mutation_config(extra: &str) -> String {
    BASE_CONFIG.replace(
        "[mutation]\nenabled = false",
        &format!("[mutation]\nenabled = true\nmin_score = 0.0\ntimeout_secs = 2\n{extra}"),
    )
}

fn source_fixture(prefix: &str, config: &str, path: &str, content: &str) -> FixtureRoot {
    let root = FixtureRoot::new(prefix);
    write(&root, "hardgate.toml", config);
    write(&root, path, content);
    root
}

fn mutate(root: &Path, path: &str, test_cmd: &str, max_mutants: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hardgate"));
    command
        .args(["mutate", "--scoped", path, "--test-cmd", test_cmd])
        .current_dir(root);
    if let Some(maximum) = max_mutants {
        command.args(["--max-mutants", maximum]);
    }
    command.output().expect("hardgate mutate should run")
}

fn assert_stdout_failure(output: &Output, expected: &[&str]) {
    assert!(!output.status.success(), "command unexpectedly passed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for needle in expected {
        assert!(stdout.contains(needle), "missing `{needle}` in {stdout}");
    }
}

#[test]
fn diff_clone_uses_full_repository_index() {
    let root = FixtureRoot::new("p0-diff-clone");
    write(&root, "hardgate.toml", BASE_CONFIG);
    let copied = r#"
fn calculate_total(values: &[i32]) -> i32 {
    let mut total = 0;
    for value in values {
        if *value > 0 {
            total += *value;
        }
    }
    total
}
"#;
    write(&root, "src/original.rs", copied);
    init_repo(&root);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "baseline"]);
    write(&root, "src/copied.rs", copied);

    let output = hardgate(&root, &["check", "--diff", "--format", "json"]);
    assert_stdout_failure(
        &output,
        &["clone_violations", "src/copied.rs", "src/original.rs"],
    );
}

#[test]
fn budget_exclusion_does_not_hide_other_engines() {
    let root = FixtureRoot::new("p0-exclusion-ownership");
    let config = format!(
        "{BASE_CONFIG}\n[budgets.files.exclusions]\npaths = [\"src/excluded/**\"]\n\n[invariants]\nenforce = true\n\n[[invariants.rules]]\nname = \"forbidden-import\"\nfrom = \"src/excluded/**\"\ndisallow_tokens = [\"forbidden\"]\nmessage = \"forbidden import\"\n"
    );
    write(&root, "hardgate.toml", &config);
    write(
        &root,
        "src/excluded/bad.rs",
        "#[allow(dead_code)]\nuse forbidden::thing;\nfn bad() {}\n",
    );

    let output = hardgate(&root, &["check", "--format", "json"]);
    assert_stdout_failure(
        &output,
        &[
            "allow(dead_code)",
            "forbidden-import",
            "excluded from file budget",
        ],
    );
}

#[test]
fn disabled_evidence_engines_ignore_stale_reports() {
    let root = FixtureRoot::new("p0-disabled-evidence");
    let config = BASE_CONFIG.replace(
        "[coverage]\nenabled = false\n\n[mutation]\nenabled = false",
        "[coverage]\nenabled = false\nreport = \"stale.lcov\"\n\n[mutation]\nenabled = false\nreports = [\"stale.json\"]",
    );
    write(&root, "hardgate.toml", &config);
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    write(&root, "stale.lcov", "not lcov\n");
    write(&root, "stale.json", "not json\n");

    let output = hardgate(&root, &["verify", "--format", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"coverage_violations\": []"), "{stdout}");
    assert!(stdout.contains("\"mutation_violations\": []"), "{stdout}");
}

#[test]
fn mutate_honors_disabled_engine_without_running_a_command() {
    let root = source_fixture(
        "p0-mutation-disabled",
        BASE_CONFIG,
        "src/lib.rs",
        "pub fn accepts(value: bool) -> bool { value == true }\n",
    );
    let output = mutate(
        &root,
        "src/lib.rs",
        "hardgate-command-that-must-not-run",
        None,
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("mutation testing is disabled"));
}

#[test]
fn nonexistent_mutation_command_fails_during_baseline() {
    let root = source_fixture(
        "p0-mutation-missing-command",
        &mutation_config(""),
        "src/lib.rs",
        "pub fn accepts(value: bool) -> bool { value == true }\n",
    );
    let output = mutate(
        &root,
        "src/lib.rs",
        "hardgate-command-that-does-not-exist",
        Some("1"),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unmutated baseline RunnerError"),
        "{stderr}"
    );
    assert!(stderr.contains("Failed to execute"), "{stderr}");
}

#[test]
fn failing_baseline_stops_before_any_mutant_runs() {
    let source = "pub fn accepts(value: bool) -> bool { value == true }\n";
    let root = source_fixture(
        "p0-mutation-baseline",
        &mutation_config(""),
        "src/lib.rs",
        source,
    );
    let output = mutate(
        &root,
        "src/lib.rs",
        "sh -c 'printf x >> baseline-runs; exit 1'",
        Some("1"),
    );
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(root.join("baseline-runs")).unwrap(),
        "x"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
        source
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unmutated baseline Failed"), "{stderr}");
}

#[test]
fn mutate_rejects_non_production_scopes_before_execution() {
    let root = source_fixture(
        "p0-mutation-role",
        &mutation_config(""),
        "tests/example.rs",
        "fn accepts(value: bool) -> bool { value == true }\n",
    );
    let output = mutate(
        &root,
        "tests/example.rs",
        "sh -c 'printf ran > mutation-command-ran'",
        None,
    );
    assert!(!output.status.success());
    assert!(!root.join("mutation-command-ran").exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("classified as Test"), "{stderr}");
}

#[test]
fn zero_viable_mutants_is_not_a_green_run() {
    let root = source_fixture(
        "p0-mutation-zero-viable",
        &mutation_config(""),
        "src/lib.rs",
        "pub fn answer() -> i32 { 42 }\n",
    );
    let output = mutate(&root, "src/lib.rs", "sh -c 'exit 0'", None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no viable AST mutation points"), "{stderr}");
}

#[test]
fn strict_missing_report_and_parser_error_fail() {
    let missing = FixtureRoot::new("p0-missing-report");
    let config = BASE_CONFIG.replace(
        "[coverage]\nenabled = false",
        "[coverage]\nenabled = true\nreport = \"missing.lcov\"",
    );
    write(&missing, "hardgate.toml", &config);
    write(&missing, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    let output = hardgate(&missing, &["verify", "--format", "json"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("coverage-report"));

    let malformed = FixtureRoot::new("p0-parser-error");
    write(&malformed, "hardgate.toml", BASE_CONFIG);
    write(&malformed, "src/lib.rs", "pub fn broken( {\n");
    let output = hardgate(&malformed, &["check", "--format", "json"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("parse-source"));
}

#[cfg(unix)]
#[test]
fn strict_unreadable_source_is_not_silently_dropped() {
    use std::os::unix::fs::PermissionsExt;

    let root = FixtureRoot::new("p0-unreadable-source");
    write(&root, "hardgate.toml", BASE_CONFIG);
    let source = root.join("src/private.rs");
    write(&root, "src/private.rs", "pub fn hidden() {}\n");
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_to_string(&source).is_ok() {
        // Root-like test environments can bypass mode bits; the branch is
        // covered on ordinary CI users and cleanup must remain possible.
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600)).unwrap();
        return;
    }

    let output = hardgate(&root, &["check", "--format", "json"]);
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("read-source"));
}

#[test]
fn diff_mode_fails_when_git_evidence_is_unavailable() {
    let root = FixtureRoot::new("p0-no-git");
    write(&root, "hardgate.toml", BASE_CONFIG);
    write(&root, "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    let output = hardgate(&root, &["check", "--diff"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("git status"), "{stderr}");
}

#[test]
fn default_classifier_covers_node_and_supabase_roles() {
    let cases = [
        ("src/app.mts", FileRole::Source, true),
        ("src/app.cts", FileRole::Source, true),
        ("src/view.stories.tsx", FileRole::Test, true),
        ("tests/__fixtures__/state.snap", FileRole::Fixture, false),
        ("src/__mocks__/client.ts", FileRole::Test, true),
        ("supabase/database.types.ts", FileRole::Generated, true),
        ("supabase/functions/mail/index.ts", FileRole::Source, true),
        (
            "supabase/migrations/001_init.sql",
            FileRole::Migration,
            false,
        ),
        ("supabase/seed.sql", FileRole::Migration, false),
        ("docs/page.mdx", FileRole::Documentation, false),
        ("schema.graphql", FileRole::Source, false),
        ("package.json", FileRole::Config, false),
    ];
    for (path, role, ast_supported) in cases {
        let classified = ClassifiedFile::new(Path::new(path));
        assert_eq!(classified.role, role, "{path}");
        assert_eq!(classified.ast_supported, ast_supported, "{path}");
    }
}
