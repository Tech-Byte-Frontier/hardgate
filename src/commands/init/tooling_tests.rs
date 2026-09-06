use super::*;

fn fixture(label: &str, tools: &[&str]) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "hardgate-tool-selection-{label}-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("node_modules/.bin")).unwrap();
    for tool in tools {
        let path = root.join("node_modules/.bin").join(tool);
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    root
}

#[test]
fn formatter_and_linter_are_selected_independently() {
    let root = fixture("independent", &["prettier", "oxlint"]);
    fs::write(root.join(".prettierrc"), "{}").unwrap();
    fs::write(root.join(".oxlintrc.json"), "{}").unwrap();
    let mut config = OrchestrationConfig::default();
    assert!(set_javascript_config_commands(&root, &mut config).is_empty());
    assert_eq!(config.format_check.as_deref(), Some("prettier --check ."));
    assert_eq!(config.lint.as_deref(), Some("oxlint ."));
    fs::write(root.join("biome.json"), "{}").unwrap();
    let mut ambiguous = OrchestrationConfig::default();
    let missing = set_javascript_config_commands(&root, &mut ambiguous);
    assert_eq!(missing.len(), 2);
    assert!(
        missing
            .iter()
            .all(|message| message.contains("multiple configurations"))
    );
    assert!(ambiguous.format_check.is_none() && ambiguous.lint.is_none());
    // Existing policy resolves each role separately.
    assert!(set_javascript_config_commands(&root, &mut config).is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn default_tools_require_installation_without_silent_fallback() {
    let root = fixture("default", &["oxfmt", "oxlint"]);
    let mut config = OrchestrationConfig::default();
    assert!(set_javascript_config_commands(&root, &mut config).is_empty());
    assert_eq!(config.format_check.as_deref(), Some("oxfmt --check ."));
    fs::write(root.join(".prettierrc"), "{}").unwrap();
    let mut missing = OrchestrationConfig::default();
    let messages = set_javascript_config_commands(&root, &mut missing);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Prettier requires"));
    assert!(missing.format_check.is_none());
    assert_eq!(missing.lint.as_deref(), Some("oxlint ."));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn generated_script_checks_remove_write_modes_and_reject_custom_scope() {
    assert_eq!(
        script_commands("prettier --write .", true).unwrap().0,
        "prettier --check ."
    );
    assert_eq!(
        script_commands("eslint --fix .", false).unwrap().0,
        "eslint --no-fix ."
    );
    for script in [
        "eslint src",
        "eslint . && touch source.ts",
        "node lint.js",
        "npm run lint:fix",
    ] {
        assert!(script_commands(script, false).is_none(), "{script}");
    }
}
