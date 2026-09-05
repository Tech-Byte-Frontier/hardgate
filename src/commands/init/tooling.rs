use super::detect::has_any;
use crate::config::OrchestrationConfig;
use std::path::Path;

pub(crate) fn set_javascript_config_commands(root: &Path, orchestration: &mut OrchestrationConfig) {
    set_pair_if_configured(
        root,
        &["biome.json", "biome.jsonc"],
        orchestration,
        ("biome format --write=false .", "biome format --write ."),
    );
    set_single_if_configured(
        root,
        &[
            "eslint.config.js",
            "eslint.config.mjs",
            ".eslintrc",
            ".eslintrc.json",
            ".eslintrc.js",
        ],
        &mut orchestration.lint,
        "eslint .",
    );
    set_single_if_configured(
        root,
        &["oxlint.config.js"],
        &mut orchestration.lint,
        "oxlint .",
    );
    set_pair_if_configured(
        root,
        &[
            "prettier.config.js",
            "prettier.config.cjs",
            "prettier.config.mjs",
            ".prettierrc",
            ".prettierrc.json",
        ],
        orchestration,
        ("prettier --check .", "prettier --write ."),
    );
}

type CommandPair<'a> = (&'a str, &'a str);

fn set_pair_if_configured(
    root: &Path,
    names: &[&str],
    orchestration: &mut OrchestrationConfig,
    commands: CommandPair<'_>,
) {
    if has_any(root, names) {
        set_if_missing(&mut orchestration.format_check, commands.0);
        set_if_missing(&mut orchestration.format, commands.1);
    }
}

fn set_single_if_configured(
    root: &Path,
    names: &[&str],
    command: &mut Option<String>,
    value: &str,
) {
    if has_any(root, names) {
        set_if_missing(command, value);
    }
}

fn set_if_missing(command: &mut Option<String>, value: &str) {
    if command.is_none() {
        *command = Some(value.to_string());
    }
}
