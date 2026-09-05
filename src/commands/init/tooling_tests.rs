use super::*;
use std::fs;

fn scratch(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "hardgate-init-tooling-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_config_files(root: &std::path::Path) {
    fs::write(root.join("biome.json"), "{}").unwrap();
    fs::write(root.join("eslint.config.js"), "export default {};").unwrap();
    fs::write(root.join("oxlint.config.js"), "export default {};").unwrap();
    fs::write(root.join("prettier.config.js"), "module.exports = {};\n").unwrap();
}

#[test]
fn configured_tools_require_files_and_report_each_missing_tool() {
    let root = scratch("missing");
    assert!(set_javascript_config_commands(&root, &mut OrchestrationConfig::default()).is_empty());

    write_config_files(&root);
    let mut orchestration = OrchestrationConfig::default();
    let missing = set_javascript_config_commands(&root, &mut orchestration);
    for tool in ["Biome", "ESLint", "Oxlint", "Prettier"] {
        assert!(
            missing.iter().any(|message| message.starts_with(tool)),
            "missing diagnostic for {tool}: {missing:?}"
        );
    }
    assert!(orchestration.format_check.is_none());
    assert!(orchestration.format.is_none());
    assert!(orchestration.lint.is_none());

    fs::create_dir_all(root.join("node_modules/.bin/biome")).unwrap();
    assert!(local_executable(&root, "biome").is_none());
    fs::write(root.join("node_modules/.bin/eslint"), "not executable").unwrap();
    #[cfg(unix)]
    assert!(local_executable(&root, "eslint").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn executable_local_tools_fill_only_missing_commands() {
    let root = scratch("available");
    write_config_files(&root);
    let bin = root.join("node_modules/.bin");
    fs::create_dir_all(&bin).unwrap();
    for tool in ["biome", "eslint", "oxlint", "prettier"] {
        let path = bin.join(tool);
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }
    }

    let mut orchestration = OrchestrationConfig {
        format_check: Some("script format check".to_string()),
        format: None,
        lint: None,
        test_cmd: None,
        timeout_secs: None,
    };
    let missing = set_javascript_config_commands(&root, &mut orchestration);
    assert!(
        missing.is_empty(),
        "unexpected setup diagnostics: {missing:?}"
    );
    assert_eq!(
        orchestration.format_check.as_deref(),
        Some("script format check")
    );
    assert_eq!(
        orchestration.format.as_deref(),
        Some("biome format --write .")
    );
    assert_eq!(orchestration.lint.as_deref(), Some("eslint ."));

    let mut already_configured = OrchestrationConfig {
        format_check: Some("format-check".to_string()),
        format: Some("format".to_string()),
        lint: Some("lint".to_string()),
        test_cmd: None,
        timeout_secs: None,
    };
    let missing = set_javascript_config_commands(&root, &mut already_configured);
    assert!(missing.is_empty());
    assert_eq!(
        already_configured.format_check.as_deref(),
        Some("format-check")
    );
    assert_eq!(already_configured.format.as_deref(), Some("format"));
    assert_eq!(already_configured.lint.as_deref(), Some("lint"));
    assert!(local_executable(&root, "biome").is_some());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn single_tool_commands_respect_existing_values() {
    let root = scratch("single-existing");
    fs::write(root.join("eslint.config.js"), "export default {};\n").unwrap();
    let mut command = Some("configured lint".to_string());
    let mut missing = Vec::new();
    set_single_if_available(
        &root,
        SingleSpec {
            names: &["eslint.config.js"],
            command: "eslint .",
            tool: "ESLint",
        },
        &mut command,
        &mut missing,
    );
    assert_eq!(command.as_deref(), Some("configured lint"));
    assert!(missing.is_empty());

    let mut command = None;
    set_if_missing(&mut command, "first");
    set_if_missing(&mut command, "second");
    assert_eq!(command.as_deref(), Some("first"));
    fs::remove_dir_all(root).unwrap();
}
