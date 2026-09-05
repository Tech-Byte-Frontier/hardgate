#[path = "support/init_experience.rs"]
mod support;

use hardgate::commands::init::{cmd_init, cmd_init_with_options};
use hardgate::config::{HardgateConfig, Preset};
use std::fs;
use std::path::Path;
use support::{assert_commands, load_written, options, with_root};

fn write(root: &Path, name: &str, content: &str) {
    fs::write(root.join(name), content).unwrap();
}

fn balanced_case(
    tag: &str,
    setup: impl FnOnce(&Path),
    check: impl FnOnce(&Path, &HardgateConfig, &str),
) {
    with_root(tag, |root| {
        setup(root);
        cmd_init_with_options(options("balanced")).unwrap();
        let config = load_written(root);
        let content = fs::read_to_string(root.join("hardgate.toml")).unwrap();
        check(root, &config, &content);
    });
}

fn assert_unconfigured(config: &HardgateConfig, content: &str, marker: &str) {
    assert!(config.orchestration.format_check.is_none());
    assert!(config.orchestration.format.is_none());
    assert!(config.orchestration.lint.is_none());
    if !marker.is_empty() {
        assert!(content.contains(marker), "missing diagnostic: {marker}");
    }
}

#[test]
fn historical_api_and_unknown_preset_use_strict_defaults() {
    with_root("compatibility-fallback", |root| {
        cmd_init("UNKNOWN-PRESET").unwrap();
        let config = load_written(root);
        assert_eq!(config.gate.preset, Preset::StrictAgent);
        assert!(config.coverage.enabled);
        assert!(config.mutation.enabled);
    });
}

#[test]
fn malformed_and_unsupported_javascript_metadata_stays_unconfigured() {
    balanced_case(
        "javascript-malformed",
        |root| write(root, "package.json", "{\"scripts\":"),
        |_, config, content| {
            assert_unconfigured(
                config,
                content,
                "package.json could not be read as a JSON object",
            )
        },
    );
    balanced_case(
        "javascript-unsupported-manager",
        |root| {
            write(
                root,
                "package.json",
                r#"{"packageManager":"deno@2","scripts":{"format":"format","lint":"lint"}}"#,
            )
        },
        |_, config, content| assert_unconfigured(config, content, "unsupported package manager"),
    );
    balanced_case(
        "javascript-invalid-scripts",
        |root| {
            write(
                root,
                "package.json",
                r#"{"packageManager":"npm@10","scripts":{"format":"   ","lint":false,"test":42}}"#,
            )
        },
        |_, config, content| {
            assert_unconfigured(config, content, "");
            assert!(config.orchestration.test_cmd.is_none());
        },
    );
}

#[test]
fn javascript_package_scripts_supply_all_orchestration_commands() {
    balanced_case(
        "javascript-package-scripts",
        |root| {
            write(
                root,
                "package.json",
                r#"{"packageManager":"npm@10","scripts":{"format:check":"format-check","format":"format","lint":"lint","test":"test"}}"#,
            )
        },
        |_, config, _| {
            assert_commands(
                config,
                [
                    "npm run format:check",
                    "npm run format",
                    "npm run lint",
                    "npm run test",
                ],
            );
        },
    );
}

#[test]
fn nested_and_deep_manifests_do_not_create_root_commands() {
    balanced_case(
        "nested-javascript",
        |root| {
            let package = root.join("packages/web");
            fs::create_dir_all(&package).unwrap();
            fs::write(
                package.join("package.json"),
                r#"{"packageManager":"pnpm@9","scripts":{"format":"format","lint":"lint"}}"#,
            )
            .unwrap();
        },
        |_, config, content| {
            assert_unconfigured(
                config,
                content,
                "package.json was found below the policy root",
            )
        },
    );
    balanced_case(
        "deep-manifest",
        |root| {
            let package = root.join("one/two/three/four");
            fs::create_dir_all(&package).unwrap();
            write(&package, "package.json", r#"{"scripts":{"lint":"lint"}}"#);
        },
        |_, config, content| {
            assert_unconfigured(config, content, "");
            assert!(content.contains("no supported manifest or configured formatter"));
        },
    );
}

#[cfg(unix)]
#[test]
fn symlinked_manifest_trees_are_not_scanned() {
    balanced_case(
        "symlinked-manifest",
        |root| {
            let real = root
                .parent()
                .unwrap()
                .join(format!("hardgate-init-real-package-{}", std::process::id()));
            let _ = fs::remove_dir_all(&real);
            fs::create_dir_all(&real).unwrap();
            write(
                &real,
                "package.json",
                r#"{"scripts":{"format":"format","lint":"lint"}}"#,
            );
            std::os::unix::fs::symlink(&real, root.join("linked-package")).unwrap();
        },
        |root, config, content| {
            assert_unconfigured(
                config,
                content,
                "no supported manifest or configured formatter",
            );
            let real = root
                .parent()
                .unwrap()
                .join(format!("hardgate-init-real-package-{}", std::process::id()));
            fs::remove_dir_all(real).unwrap();
        },
    );
}

#[test]
fn javascript_config_without_a_local_tool_stays_unconfigured() {
    balanced_case(
        "javascript-config-missing-tool",
        |root| write(root, "biome.json", "{}"),
        |_, config, content| {
            assert_unconfigured(config, content, "Biome configuration was detected")
        },
    );
}

#[test]
fn javascript_config_rejects_a_directory_as_the_tool() {
    balanced_case(
        "javascript-config-directory-tool",
        |root| {
            write(root, "biome.json", "{}");
            fs::create_dir_all(root.join("node_modules/.bin/biome")).unwrap();
        },
        |_, config, _| {
            assert!(config.orchestration.format_check.is_none());
            assert!(config.orchestration.lint.is_none());
        },
    );
}

#[cfg(unix)]
#[test]
fn javascript_config_accepts_a_validated_local_executable() {
    balanced_case(
        "javascript-config-tool",
        |root| {
            write(root, "biome.json", "{}");
            let bin = root.join("node_modules/.bin");
            fs::create_dir_all(&bin).unwrap();
            let tool = bin.join("biome");
            write(&bin, "biome", "#!/bin/sh\nexit 0\n");
            let mut permissions = fs::metadata(tool).unwrap().permissions();
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o755);
            fs::set_permissions(bin.join("biome"), permissions).unwrap();
        },
        |_, config, _| {
            assert_eq!(
                config.orchestration.format_check.as_deref(),
                Some("biome ci --linter-enabled=false .")
            );
            assert_eq!(
                config.orchestration.format.as_deref(),
                Some("biome format --write .")
            );
            assert_eq!(
                config.orchestration.lint.as_deref(),
                Some("biome ci --formatter-enabled=false .")
            );
        },
    );
}

#[test]
fn python_config_and_invalid_toml_are_reported_without_guessing() {
    balanced_case(
        "python-invalid-toml",
        |root| write(root, "pyproject.toml", "[tool.ruff\n"),
        |_, config, content| {
            assert!(config.orchestration.format.is_none());
            assert!(config.orchestration.lint.is_none());
            assert!(content.contains("formatter command is not configured"));
            assert!(content.contains("linter command is not configured"));
        },
    );
    balanced_case(
        "python-ruff-config",
        |root| write(root, "ruff.toml", "line-length = 88\n"),
        |_, config, _| {
            assert_eq!(
                config.orchestration.format_check.as_deref(),
                Some("ruff format --check .")
            );
            assert_eq!(config.orchestration.lint.as_deref(), Some("ruff check ."));
        },
    );
    balanced_case(
        "python-test-config",
        |root| write(root, "tox.ini", "[tox]\nenvlist = py\n"),
        |_, config, _| {
            assert_eq!(config.orchestration.test_cmd.as_deref(), Some("pytest"));
            assert!(config.orchestration.format.is_none());
            assert!(config.orchestration.lint.is_none());
        },
    );
}

#[test]
fn package_manager_lockfiles_are_explicit_and_deduplicated() {
    balanced_case(
        "javascript-lock-ambiguity",
        |root| {
            write(
                root,
                "package.json",
                r#"{"scripts":{"format":"format","lint":"lint"}}"#,
            );
            write(root, "pnpm-lock.yaml", "lockfileVersion: 9\n");
            write(root, "package-lock.json", "{}\n");
        },
        |_, config, content| {
            assert!(config.orchestration.format.is_none());
            assert!(config.orchestration.lint.is_none());
            assert!(content.contains("multiple package manager lockfiles"));
        },
    );
    balanced_case(
        "javascript-bun-lockfiles",
        |root| {
            write(
                root,
                "package.json",
                r#"{"scripts":{"format":"format","lint":"lint"}}"#,
            );
            write(root, "bun.lock", "lockfileVersion = 1\n");
            write(root, "bun.lockb", "legacy\n");
        },
        |_, config, _| {
            assert_eq!(
                config.orchestration.format.as_deref(),
                Some("bun run format")
            );
            assert_eq!(config.orchestration.lint.as_deref(), Some("bun run lint"));
        },
    );
}

#[test]
fn strict_setup_reports_real_coverage_and_mutation_requirements() {
    with_root("strict-existing-coverage", |root| {
        fs::create_dir_all(root.join("coverage")).unwrap();
        write(root, "coverage/lcov.info", "SF:src/lib.rs\n");
        cmd_init_with_options(options("strict-agent")).unwrap();
        let content = fs::read_to_string(root.join("hardgate.toml")).unwrap();
        assert!(!content.contains("coverage report coverage/lcov.info is not present yet"));
        assert!(content.contains("configure mutation.reports"));
        assert!(content.contains("hardgate check also requires"));
    });
}
