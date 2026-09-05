#[path = "support/init_experience.rs"]
mod support;

use hardgate::commands::init::{InitOptions, cmd_init_with_options};
use hardgate::config::HardgateConfig;
use std::fs;
use support::{assert_commands, load_written, options, with_root};

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
            if preset == "strict-agent" {
                assert!(content.contains("mutation.reports"));
                assert!(content.contains("95% line/function"));
                assert!(content.contains("remains incomplete until real LCOV"));
                assert!(content.contains("hardgate check also requires"));
            }
            if preset == "balanced" {
                assert!(content.contains("structural starting point"));
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

fn generated_policy(
    tag: &str,
    preset: &str,
    manifest: Option<(&str, &str)>,
    generation: (bool, [Option<&str>; 3]),
) -> HardgateConfig {
    let (full, overrides) = generation;
    let mut generated = None;
    with_root(tag, |root| {
        if let Some((name, content)) = manifest {
            fs::write(root.join(name), content).unwrap();
        }
        let mut init = options(preset);
        init.full = full;
        init.format_check = overrides[0].map(str::to_string);
        init.format = overrides[1].map(str::to_string);
        init.lint = overrides[2].map(str::to_string);
        cmd_init_with_options(init).unwrap();
        generated = Some(load_written(root));
    });
    generated.unwrap()
}

fn assert_policy_round_trip(
    tag: &str,
    preset: &str,
    manifest: Option<(&str, &str)>,
    overrides: [Option<&str>; 3],
) {
    let full = generated_policy(tag, preset, manifest, (true, overrides));
    let concise = generated_policy(tag, preset, manifest, (false, overrides));
    assert_eq!(
        serde_json::to_value(full).unwrap(),
        serde_json::to_value(concise).unwrap(),
        "full and concise policies diverged for {tag}"
    );
}

#[test]
fn concise_and_full_outputs_preserve_the_effective_policy() {
    for preset in ["strict-agent", "balanced", "legacy-migration", "custom"] {
        assert_policy_round_trip(
            &format!("preset-{preset}"),
            preset,
            None,
            [None, None, None],
        );
    }
    for (tag, manifest) in [
        (
            "effective-rust",
            (
                "Cargo.toml",
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
            ),
        ),
        (
            "effective-javascript",
            (
                "package.json",
                r#"{"packageManager":"pnpm@9","scripts":{"format":"format","lint":"lint","test":"test"}}"#,
            ),
        ),
        (
            "effective-python",
            (
                "pyproject.toml",
                "[project]\nname = \"fixture\"\n\n[tool.ruff]\nline-length = 88\n",
            ),
        ),
        (
            "effective-go",
            ("go.mod", "module example.test\n\ngo 1.23\n"),
        ),
    ] {
        assert_policy_round_trip(tag, "balanced", Some(manifest), [None, None, None]);
    }
    assert_policy_round_trip(
        "effective-overrides",
        "balanced",
        Some(("go.mod", "module example.test\n\ngo 1.23\n")),
        [
            Some("tool format --check"),
            Some("tool format"),
            Some("tool lint"),
        ],
    );
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
fn root_python_metadata_wins_over_nested_inventory_order() {
    with_root("python-root-first", |root| {
        fs::write(
            root.join("pyproject.toml"),
            "[project]\nname = \"root\"\n\n[tool.black]\nline-length = 88\n",
        )
        .unwrap();
        let nested = root.join("packages").join("app");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("pyproject.toml"),
            "[project]\nname = \"nested\"\n\n[tool.ruff]\nline-length = 100\n",
        )
        .unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let config = load_written(root);
        assert_eq!(
            config.orchestration.format_check.as_deref(),
            Some("black --check .")
        );
        assert_eq!(config.orchestration.format.as_deref(), Some("black ."));
        assert!(config.orchestration.lint.is_none());
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
            [
                "sh -c 'files=$(gofmt -l .) || exit $?; test -z \"$files\"'",
                "gofmt -w .",
                "go vet ./...",
                "go test ./...",
            ],
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
        let content = fs::read_to_string(root.join("hardgate.toml")).unwrap();
        assert_eq!(
            config.orchestration.format_check.as_deref(),
            Some("tool format --check")
        );
        assert_eq!(config.orchestration.format.as_deref(), Some("tool format"));
        assert_eq!(config.orchestration.lint.as_deref(), Some("tool lint"));
        assert!(!content.contains("formatter command is not configured"));
        assert!(!content.contains("linter command is not configured"));
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
