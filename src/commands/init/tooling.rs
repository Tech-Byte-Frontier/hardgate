use super::detect::has_any;
use crate::config::OrchestrationConfig;
use std::fs;
use std::path::Path;

pub(crate) fn set_javascript_config_commands(
    root: &Path,
    orchestration: &mut OrchestrationConfig,
) -> Vec<String> {
    let mut missing = Vec::new();
    set_pair_if_available(
        root,
        PairSpec {
            names: &["biome.json", "biome.jsonc"],
            commands: (
                "biome ci --linter-enabled=false .",
                "biome format --write .",
            ),
            tool: "Biome",
        },
        orchestration,
        &mut missing,
    );
    set_single_if_available(
        root,
        SingleSpec {
            names: &["biome.json", "biome.jsonc"],
            command: "biome ci --formatter-enabled=false .",
            tool: "Biome",
        },
        &mut orchestration.lint,
        &mut missing,
    );
    set_single_if_available(
        root,
        SingleSpec {
            names: &[
                "eslint.config.js",
                "eslint.config.mjs",
                ".eslintrc",
                ".eslintrc.json",
                ".eslintrc.js",
            ],
            command: "eslint .",
            tool: "ESLint",
        },
        &mut orchestration.lint,
        &mut missing,
    );
    set_single_if_available(
        root,
        SingleSpec {
            names: &["oxlint.config.js"],
            command: "oxlint .",
            tool: "Oxlint",
        },
        &mut orchestration.lint,
        &mut missing,
    );
    set_pair_if_available(
        root,
        PairSpec {
            names: &[
                "prettier.config.js",
                "prettier.config.cjs",
                "prettier.config.mjs",
                ".prettierrc",
                ".prettierrc.json",
            ],
            commands: ("prettier --check .", "prettier --write ."),
            tool: "Prettier",
        },
        orchestration,
        &mut missing,
    );
    missing
}

struct PairSpec<'a> {
    names: &'a [&'a str],
    commands: (&'a str, &'a str),
    tool: &'a str,
}

struct SingleSpec<'a> {
    names: &'a [&'a str],
    command: &'a str,
    tool: &'a str,
}

fn set_pair_if_available(
    root: &Path,
    spec: PairSpec<'_>,
    orchestration: &mut OrchestrationConfig,
    missing: &mut Vec<String>,
) {
    if !has_any(root, spec.names) {
        return;
    }
    if orchestration.format_check.is_some() && orchestration.format.is_some() {
        return;
    }
    let executable = spec.tool.to_ascii_lowercase();
    if local_executable(root, &executable).is_none() {
        missing_tool(spec.tool, missing);
        return;
    }
    set_if_missing(&mut orchestration.format_check, spec.commands.0);
    set_if_missing(&mut orchestration.format, spec.commands.1);
}

fn set_single_if_available(
    root: &Path,
    spec: SingleSpec<'_>,
    command: &mut Option<String>,
    missing: &mut Vec<String>,
) {
    if !has_any(root, spec.names) || command.is_some() {
        return;
    }
    let executable = spec.tool.to_ascii_lowercase();
    if local_executable(root, &executable).is_some() {
        *command = Some(spec.command.to_string());
    } else {
        missing_tool(spec.tool, missing);
    }
}

fn missing_tool(tool: &str, missing: &mut Vec<String>) {
    missing.push(format!(
        "{tool} configuration was detected, but the repository-local executable is unavailable; install declared dependencies or provide an explicit [orchestration] command"
    ));
}

fn local_executable(root: &Path, tool: &str) -> Option<()> {
    let path = root.join("node_modules").join(".bin").join(tool);
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o111 == 0 {
        return None;
    }
    Some(())
}

fn set_if_missing(command: &mut Option<String>, value: &str) {
    if command.is_none() {
        *command = Some(value.to_string());
    }
}
