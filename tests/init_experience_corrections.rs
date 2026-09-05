#[path = "support/init_experience.rs"]
mod support;

use hardgate::commands::init::cmd_init_with_options;
use std::fs;
use std::process::Command;
use support::{WorkingDirectory, assert_commands, load_written, options, with_root};

fn assert_nested_case(tag: &str, manifest: &str, manifest_content: &str, expected: [&str; 4]) {
    with_root(tag, |root| {
        let nested = root.join("packages").join("app");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join(manifest), manifest_content).unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let root_config = load_written(root);
        assert!(root_config.orchestration.format_check.is_none());
        assert!(root_config.orchestration.lint.is_none());
        assert!(
            fs::read_to_string(root.join("hardgate.toml"))
                .unwrap()
                .contains("initialize inside that package")
        );

        let directory = WorkingDirectory::enter(&nested);
        cmd_init_with_options(options("balanced")).unwrap();
        drop(directory);
        let nested_config = load_written(&nested);
        assert_commands(&nested_config, expected);
    });
}

#[test]
fn nested_manifests_are_initialized_at_the_package_root() {
    assert_nested_case(
        "nested-rust",
        "Cargo.toml",
        "[package]\nname = \"nested\"\nversion = \"0.1.0\"\n",
        [
            "cargo fmt --all -- --check",
            "cargo fmt --all",
            "cargo clippy --all-targets --all-features -- -D warnings",
            "cargo test --all-targets",
        ],
    );
    assert_nested_case(
        "nested-python",
        "pyproject.toml",
        "[project]\nname = \"nested\"\n\n[tool.ruff]\nline-length = 88\n\n[tool.pytest.ini_options]\naddopts = \"-q\"\n",
        [
            "ruff format --check .",
            "ruff format .",
            "ruff check .",
            "pytest",
        ],
    );
    assert_nested_case(
        "nested-go",
        "go.mod",
        "module nested.test\n\ngo 1.23\n",
        [
            "sh -c 'files=$(gofmt -l .) || exit $?; test -z \"$files\"'",
            "gofmt -w .",
            "go vet ./...",
            "go test ./...",
        ],
    );
}

#[test]
fn nested_javascript_scripts_are_initialized_at_the_package_root() {
    with_root("nested-javascript", |root| {
        let nested = root.join("packages").join("app");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("package.json"),
            r#"{"packageManager":"npm@10","scripts":{"format":"format","lint":"lint","test":"test"}}"#,
        )
        .unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        assert!(load_written(root).orchestration.format.is_none());

        let directory = WorkingDirectory::enter(&nested);
        cmd_init_with_options(options("balanced")).unwrap();
        drop(directory);
        let config = load_written(&nested);
        assert_eq!(
            config.orchestration.format.as_deref(),
            Some("npm run format")
        );
        assert_eq!(config.orchestration.lint.as_deref(), Some("npm run lint"));
        assert_eq!(
            config.orchestration.test_cmd.as_deref(),
            Some("npm run test")
        );
    });
}

#[test]
fn javascript_config_requires_a_repository_local_executable() {
    with_root("javascript-config", |root| {
        fs::write(root.join("package.json"), r#"{"name":"fixture"}"#).unwrap();
        fs::write(root.join("prettier.config.js"), "module.exports = {};\n").unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let config = load_written(root);
        let content = fs::read_to_string(root.join("hardgate.toml")).unwrap();
        assert!(config.orchestration.format_check.is_none());
        assert!(content.contains("Prettier configuration was detected"));
    });

    #[cfg(unix)]
    with_root("javascript-config-local", |root| {
        fs::write(root.join("package.json"), r#"{"name":"fixture"}"#).unwrap();
        fs::write(root.join("prettier.config.js"), "module.exports = {};\n").unwrap();
        let bin = root.join("node_modules").join(".bin");
        fs::create_dir_all(&bin).unwrap();
        let executable = bin.join("prettier");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let config = load_written(root);
        assert_eq!(
            config.orchestration.format_check.as_deref(),
            Some("prettier --check .")
        );
        assert_eq!(
            config.orchestration.format.as_deref(),
            Some("prettier --write .")
        );
    });
}

#[test]
fn javascript_scripts_require_nonempty_values_and_one_manager() {
    with_root("javascript-invalid-scripts", |root| {
        fs::write(
            root.join("package.json"),
            r#"{"packageManager":"npm@10","scripts":{"format:check":"","format":false,"lint":[],"test":"  "}}"#,
        )
        .unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let config = load_written(root);
        assert!(config.orchestration.format_check.is_none());
        assert!(config.orchestration.format.is_none());
        assert!(config.orchestration.lint.is_none());
        assert!(config.orchestration.test_cmd.is_none());
    });

    with_root("javascript-ambiguous-manager", |root| {
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"format":"format"}}"#,
        )
        .unwrap();
        fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
        fs::write(root.join("package-lock.json"), "{}\n").unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let config = load_written(root);
        let content = fs::read_to_string(root.join("hardgate.toml")).unwrap();
        assert!(config.orchestration.format.is_none());
        assert!(content.contains("multiple package manager lockfiles"));
    });

    with_root("javascript-bun-lock-variants", |root| {
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"format":"format"}}"#,
        )
        .unwrap();
        fs::write(root.join("bun.lock"), "lockfileVersion: 1\n").unwrap();
        fs::write(root.join("bun.lockb"), "legacy\n").unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        assert_eq!(
            load_written(root).orchestration.format.as_deref(),
            Some("bun run format")
        );
    });
}

#[test]
fn preview_stdout_is_toml_and_summary_is_stderr() {
    with_root("preview-cli", |root| {
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_hardgate"))
            .args(["init", "--preset", "strict-agent", "--preview"])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let _: toml::Value = toml::from_str(&stdout).unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("summary:"));
        assert!(stderr.contains("next: hardgate config."));
        assert!(stderr.contains("strict evidence: hardgate check also requires"));
        assert!(!root.join("hardgate.toml").exists());
    });
}

#[cfg(unix)]
#[test]
fn go_format_check_fails_for_dirty_files_and_gofmt_errors() {
    use std::os::unix::fs::PermissionsExt;

    with_root("go-format-check", |root| {
        fs::write(root.join("go.mod"), "module example.test\n\ngo 1.23\n").unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let command = load_written(root).orchestration.format_check.unwrap();
        let bin = root.join("fake-bin");
        fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("gofmt");
        fs::write(
            &fake,
            "#!/bin/sh\nif [ \"$FAKE_GOFMT_MODE\" = error ]; then exit 17; fi\nprintf '%s\\n' dirty.go\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&fake).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions).unwrap();
        let mut path = bin.into_os_string();
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let run = |mode| {
            Command::new("sh")
                .arg("-c")
                .arg(&command)
                .current_dir(root)
                .env("PATH", &path)
                .env("FAKE_GOFMT_MODE", mode)
                .output()
                .unwrap()
        };
        assert!(!run("dirty").status.success());
        assert_eq!(run("error").status.code(), Some(17));
    });
}
