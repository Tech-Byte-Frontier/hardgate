use hardgate::commands::init::{InitOptions, cmd_init_with_options};
use hardgate::config::HardgateConfig;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static CURRENT_DIRECTORY: OnceLock<Mutex<()>> = OnceLock::new();

struct WorkingDirectory {
    original: PathBuf,
}

impl WorkingDirectory {
    fn enter(root: &Path) -> Self {
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(root).unwrap();
        Self { original }
    }
}

impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.original).unwrap();
    }
}

fn with_root(tag: &str, callback: impl FnOnce(&Path)) {
    let _lock = CURRENT_DIRECTORY
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "hardgate-init-experience-{}-{stamp}-{tag}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let directory = WorkingDirectory::enter(&root);
    callback(&root);
    drop(directory);
    fs::remove_dir_all(root).unwrap();
}

fn options(preset: &str) -> InitOptions {
    InitOptions {
        preset: preset.to_string(),
        ..InitOptions::default()
    }
}

fn load_written(root: &Path) -> HardgateConfig {
    HardgateConfig::load_or_default(Some(&root.join("hardgate.toml"))).unwrap()
}

fn initialize_manifest(root: &Path, name: &str, content: &str) -> HardgateConfig {
    fs::write(root.join(name), content).unwrap();
    cmd_init_with_options(options("balanced")).unwrap();
    load_written(root)
}

fn assert_commands(config: &HardgateConfig, expected: [&str; 4]) {
    assert_eq!(
        config.orchestration.format_check.as_deref(),
        Some(expected[0])
    );
    assert_eq!(config.orchestration.format.as_deref(), Some(expected[1]));
    assert_eq!(config.orchestration.lint.as_deref(), Some(expected[2]));
    assert_eq!(config.orchestration.test_cmd.as_deref(), Some(expected[3]));
}

#[test]
fn presets_round_trip_and_explain_their_first_step() {
    for (preset, coverage, mutation, ratchet) in [
        ("strict-agent", true, true, false),
        ("balanced", false, false, false),
        ("legacy-migration", false, false, true),
        ("custom", false, false, false),
    ] {
        with_root(preset, |root| {
            cmd_init_with_options(options(preset)).unwrap();
            let content = fs::read_to_string(root.join("hardgate.toml")).unwrap();
            let config = load_written(root);
            assert_eq!(config.coverage.enabled, coverage, "{preset}");
            assert_eq!(config.mutation.enabled, mutation, "{preset}");
            assert_eq!(config.legacy.ratchet, ratchet, "{preset}");
            assert!(content.contains("Detected project kind"));
            assert!(content.contains("Next step") || content.contains("next"));
            if preset == "strict-agent" {
                assert!(content.contains("mutation.reports"));
                assert!(content.contains("95% line/function"));
            }
            if preset == "legacy-migration" {
                assert!(content.contains("legacy-migration"));
                assert!(content.contains("origin/main"));
            }
            if preset == "custom" {
                assert!(content.contains("not an empty shell"));
            }
        });
    }
}

#[test]
fn full_output_contains_effective_policy_while_default_stays_concise() {
    with_root("full", |root| {
        cmd_init_with_options(InitOptions {
            full: true,
            preset: "balanced".to_string(),
            ..InitOptions::default()
        })
        .unwrap();
        let content = fs::read_to_string(root.join("hardgate.toml")).unwrap();
        assert!(content.contains("expanded effective configuration"));
        assert!(content.contains("[budgets.files]"));
        let _: HardgateConfig = toml::from_str(&content).unwrap();
    });

    with_root("concise", |root| {
        cmd_init_with_options(options("balanced")).unwrap();
        let content = fs::read_to_string(root.join("hardgate.toml")).unwrap();
        assert!(content.contains("concise preset"));
        assert!(!content.contains("[budgets.files]"));
        let _: HardgateConfig = toml::from_str(&content).unwrap();
    });
}

#[test]
fn rust_detection_uses_cargo_commands() {
    with_root("rust", |root| {
        let config = initialize_manifest(
            root,
            "Cargo.toml",
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        );
        assert_commands(
            &config,
            [
                "cargo fmt --all -- --check",
                "cargo fmt --all",
                "cargo clippy --all-targets --all-features -- -D warnings",
                "cargo test --all-targets",
            ],
        );
    });
}

#[test]
fn javascript_detection_uses_safe_package_script_wrappers() {
    with_root("javascript", |root| {
        fs::write(
            root.join("package.json"),
            r#"{
  "packageManager": "pnpm@9",
  "scripts": {
    "format:check": "touch FORMAT_EXECUTED",
    "format": "touch FORMAT_WRITE_EXECUTED",
    "lint": "touch LINT_EXECUTED",
    "test": "touch TEST_EXECUTED"
  }
}"#,
        )
        .unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let config = load_written(root);
        assert_eq!(
            config.orchestration.format_check.as_deref(),
            Some("pnpm run format:check")
        );
        assert_eq!(
            config.orchestration.format.as_deref(),
            Some("pnpm run format")
        );
        assert_eq!(config.orchestration.lint.as_deref(), Some("pnpm run lint"));
        assert_eq!(
            config.orchestration.test_cmd.as_deref(),
            Some("pnpm run test")
        );
        assert!(!root.join("FORMAT_EXECUTED").exists());
        assert!(!root.join("LINT_EXECUTED").exists());
        assert!(!root.join("TEST_EXECUTED").exists());
    });
}

#[test]
fn python_detection_requires_explicit_configured_tools() {
    with_root("python", |root| {
        let config = initialize_manifest(
            root,
            "pyproject.toml",
            "[project]\nname = \"fixture\"\n\n[tool.ruff]\nline-length = 88\n\n[tool.pytest.ini_options]\naddopts = \"-q\"\n",
        );
        assert_commands(
            &config,
            [
                "ruff format --check .",
                "ruff format .",
                "ruff check .",
                "pytest",
            ],
        );
    });
}

#[test]
fn go_detection_uses_go_tools_without_js_defaults() {
    with_root("go", |root| {
        fs::write(root.join("go.mod"), "module example.test\n\ngo 1.23\n").unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let config = load_written(root);
        assert_commands(
            &config,
            ["gofmt -l .", "gofmt -w .", "go vet ./...", "go test ./..."],
        );
        assert_ne!(
            config.orchestration.lint.as_deref(),
            Some("oxlint --type-aware .")
        );
    });
}

#[test]
fn ambiguous_monorepo_leaves_commands_unconfigured() {
    with_root("ambiguous", |root| {
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(root.join("package.json"), "{\"name\":\"fixture\"}").unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let content = fs::read_to_string(root.join("hardgate.toml")).unwrap();
        let config = load_written(root);
        assert!(config.orchestration.format_check.is_none());
        assert!(config.orchestration.lint.is_none());
        assert!(content.contains("multiple supported ecosystems"));
        assert!(content.contains("configure [orchestration] commands explicitly"));
    });
}

#[test]
fn explicit_commands_override_detection() {
    with_root("overrides", |root| {
        fs::write(root.join("go.mod"), "module example.test\n\ngo 1.23\n").unwrap();
        cmd_init_with_options(InitOptions {
            preset: "balanced".to_string(),
            format_check: Some("tool format --check".to_string()),
            format: Some("tool format".to_string()),
            lint: Some("tool lint".to_string()),
            ..InitOptions::default()
        })
        .unwrap();
        let config = load_written(root);
        assert_eq!(
            config.orchestration.format_check.as_deref(),
            Some("tool format --check")
        );
        assert_eq!(config.orchestration.format.as_deref(), Some("tool format"));
        assert_eq!(config.orchestration.lint.as_deref(), Some("tool lint"));
    });
}

#[test]
fn preview_does_not_write() {
    with_root("preview", |root| {
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        cmd_init_with_options(InitOptions {
            preset: "balanced".to_string(),
            preview: true,
            ..InitOptions::default()
        })
        .unwrap();
        assert!(!root.join("hardgate.toml").exists());
    });
}

#[test]
fn existing_file_and_broken_symlink_are_never_overwritten() {
    with_root("existing", |root| {
        fs::write(root.join("hardgate.toml"), "sentinel = true\n").unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("hardgate.toml")).unwrap(),
            "sentinel = true\n"
        );
    });

    #[cfg(unix)]
    with_root("broken-link", |root| {
        std::os::unix::fs::symlink("missing-policy.toml", root.join("hardgate.toml")).unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        assert!(
            fs::symlink_metadata(root.join("hardgate.toml"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!root.join("missing-policy.toml").exists());
    });
}
