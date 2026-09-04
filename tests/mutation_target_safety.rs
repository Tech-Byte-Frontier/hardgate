#[path = "support/fs.rs"]
mod fs;
#[path = "common/fs_git.rs"]
mod fs_git;

use fs_git::{commit_baseline, init_repo, write};
use hardgate::commands::mutate::effective_mutation_target;
use hardgate::config::{ClassificationRule, HardgateConfig};
use hardgate::discovery::FileRole;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MUTATION_CONFIG: &str = r#"
[gate]
preset = "custom"
strict = true

[mutation]
enabled = true
min_score = 0.0
timeout_secs = 1
"#;

struct FixtureRoot {
    path: PathBuf,
    cleanup: PathBuf,
}

impl FixtureRoot {
    fn new(prefix: &str) -> Self {
        let path = fs::tempdir(prefix);
        Self {
            cleanup: path.clone(),
            path,
        }
    }

    fn nested(prefix: &str, parent: &str) -> Self {
        let cleanup = fs::tempdir(prefix);
        let path = cleanup.join(parent).join("repo");
        std::fs::create_dir_all(&path).unwrap();
        Self { path, cleanup }
    }
}

impl Deref for FixtureRoot {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.cleanup);
    }
}

fn run_mutate(root: &Path, scope: Option<&Path>, diff: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hardgate"));
    command.arg("mutate");
    if diff {
        command.arg("--diff");
    }
    if let Some(scope) = scope {
        command.args(["--scoped"]).arg(scope);
    }
    command
        .args(["--test-cmd", "true", "--max-mutants", "1"])
        .current_dir(root);
    command.output().expect("hardgate mutate should run")
}

fn diagnostic(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn full_mutation_without_targets_fails_but_diff_is_an_explicit_noop() {
    let root = FixtureRoot::new("mutation-target-empty");
    write(&root, "hardgate.toml", MUTATION_CONFIG);
    init_repo(&root);
    commit_baseline(&root, "baseline");

    let full = run_mutate(&root, None, false);
    assert!(!full.status.success(), "{}", diagnostic(&full));
    assert!(
        diagnostic(&full).contains("full/native runs require at least one production target"),
        "{}",
        diagnostic(&full)
    );

    let diff = run_mutate(&root, None, true);
    assert!(diff.status.success(), "{}", diagnostic(&diff));
    assert!(diagnostic(&diff).contains("no-op"), "{}", diagnostic(&diff));
    assert!(
        diagnostic(&diff).contains("no git-modified files found"),
        "{}",
        diagnostic(&diff)
    );
}

#[test]
fn custom_source_relabels_cannot_target_protected_builtin_roles() {
    let protected = [
        ("tests/example.ts", FileRole::Test),
        ("generated/client.ts", FileRole::Generated),
        ("fixtures/state.ts", FileRole::Fixture),
        ("migrations/001_init.ts", FileRole::Migration),
        ("config/runtime.ts", FileRole::Config),
        ("vite.config.ts", FileRole::Config),
        ("docs/readme.mdx", FileRole::Documentation),
        ("vendor/library.ts", FileRole::Vendor),
    ];

    for (index, (path, role)) in protected.into_iter().enumerate() {
        let root = FixtureRoot::new(&format!("mutation-target-protected-{index}"));
        let config = format!(
            "{MUTATION_CONFIG}\n[[classification.rules]]\nglob = \"{path}\"\nrole = \"source\"\n"
        );
        write(&root, "hardgate.toml", &config);
        write(
            &root,
            path,
            "pub fn accepts(value: bool) -> bool { value == true }\n",
        );
        let output = run_mutate(&root, Some(Path::new(path)), false);
        let message = diagnostic(&output);
        assert!(!output.status.success(), "{path}: {message}");
        assert!(
            message.contains(&format!("built-in role {role:?}")),
            "{path}: {message}"
        );
    }
}

#[test]
fn unknown_builtin_paths_can_opt_into_source_only_with_source_policy() {
    let mut config = HardgateConfig::default();
    config.classification.rules.push(ClassificationRule {
        glob: "unclassified.custom".to_string(),
        role: FileRole::Source,
    });
    assert!(effective_mutation_target(Path::new("unclassified.custom"), &config).unwrap());

    config.roles.source.mutation_target = Some(false);
    assert!(!effective_mutation_target(Path::new("unclassified.custom"), &config).unwrap());

    config.roles.source.mutation_target = Some(true);
    config.classification.rules[0].role = FileRole::Test;
    config.roles.test.mutation_target = Some(true);
    assert!(!effective_mutation_target(Path::new("unclassified.custom"), &config).unwrap());
}

#[test]
fn relative_and_absolute_outside_scopes_are_rejected() {
    let root = FixtureRoot::new("mutation-target-scope-root");
    let outside = FixtureRoot::new("mutation-target-scope-outside");
    write(&root, "hardgate.toml", MUTATION_CONFIG);
    write(
        &outside,
        "outside.ts",
        "pub fn outside() -> bool { true }\n",
    );

    let relative = PathBuf::from("..")
        .join(outside.file_name().unwrap())
        .join("outside.ts");
    let relative_output = run_mutate(&root, Some(&relative), false);
    assert!(!relative_output.status.success());
    assert!(
        diagnostic(&relative_output).contains("outside repository root"),
        "{}",
        diagnostic(&relative_output)
    );

    let absolute_output = run_mutate(&root, Some(&outside.join("outside.ts")), false);
    assert!(!absolute_output.status.success());
    assert!(
        diagnostic(&absolute_output).contains("outside repository root"),
        "{}",
        diagnostic(&absolute_output)
    );
}

#[cfg(unix)]
#[test]
fn symlink_scope_resolving_outside_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = FixtureRoot::new("mutation-target-symlink-root");
    let outside = FixtureRoot::new("mutation-target-symlink-outside");
    write(&root, "hardgate.toml", MUTATION_CONFIG);
    write(
        &outside,
        "outside.ts",
        "pub fn outside() -> bool { true }\n",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    symlink(outside.join("outside.ts"), root.join("src/escape.ts")).unwrap();

    let output = run_mutate(&root, Some(Path::new("src/escape.ts")), false);
    assert!(!output.status.success());
    assert!(
        diagnostic(&output).contains("resolves outside repository root"),
        "{}",
        diagnostic(&output)
    );
}

#[cfg(unix)]
#[test]
fn absolute_symlink_alias_outside_root_can_resolve_to_an_in_root_target() {
    use std::os::unix::fs::symlink;

    let root = FixtureRoot::new("mutation-target-alias-root");
    let outside = FixtureRoot::new("mutation-target-alias-outside");
    write(&root, "hardgate.toml", MUTATION_CONFIG);
    write(
        &root,
        "src/lib.rs",
        "pub fn accepts(value: bool) -> bool { value == true }\n",
    );
    let alias = outside.join("alias.ts");
    symlink(root.join("src/lib.rs"), &alias).unwrap();

    let output = run_mutate(&root, Some(&alias), false);
    assert!(output.status.success(), "{}", diagnostic(&output));
}

#[test]
fn ancestor_directory_names_do_not_poison_relative_role_classification() {
    for (index, parent) in ["target", "tests", "config", "generated"]
        .into_iter()
        .enumerate()
    {
        let root = FixtureRoot::nested(&format!("mutation-target-ancestor-{index}"), parent);
        write(&root, "hardgate.toml", MUTATION_CONFIG);
        write(
            &root,
            "src/lib.rs",
            "pub fn accepts(value: bool) -> bool { value == true }\n",
        );

        let full = run_mutate(&root, None, false);
        assert!(full.status.success(), "{parent}: {}", diagnostic(&full));

        let scoped = run_mutate(&root, Some(Path::new("src")), false);
        assert!(scoped.status.success(), "{parent}: {}", diagnostic(&scoped));
    }
}

#[test]
fn valid_in_root_file_and_directory_scopes_are_accepted() {
    let root = FixtureRoot::new("mutation-target-valid-scope");
    write(&root, "hardgate.toml", MUTATION_CONFIG);
    write(
        &root,
        "src/lib.rs",
        "pub fn accepts(value: bool) -> bool { value == true }\n",
    );
    write(
        &root,
        "src/nested.rs",
        "pub fn nested(value: bool) -> bool { value }\n",
    );

    let file = run_mutate(&root, Some(Path::new("./src/../src/lib.rs")), false);
    assert!(file.status.success(), "{}", diagnostic(&file));

    let directory = run_mutate(&root, Some(Path::new("./src/../src")), false);
    assert!(directory.status.success(), "{}", diagnostic(&directory));
}
