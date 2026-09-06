use super::detect::has_any;
use crate::config::OrchestrationConfig;
use std::fs;
use std::path::Path;

const FORMATTERS: &[Tool] = &[
    Tool {
        name: "Oxfmt",
        executable: "oxfmt",
        configs: &[
            ".oxfmtrc.json",
            ".oxfmtrc.jsonc",
            "oxfmt.config.ts",
            "oxfmt.config.mts",
        ],
        check: "oxfmt --check .",
        fix: Some("oxfmt ."),
    },
    Tool {
        name: "Prettier",
        executable: "prettier",
        configs: &[
            "prettier.config.js",
            "prettier.config.cjs",
            "prettier.config.mjs",
            "prettier.config.ts",
            ".prettierrc",
            ".prettierrc.json",
            ".prettierrc.yaml",
            ".prettierrc.yml",
            ".prettierrc.toml",
        ],
        check: "prettier --check .",
        fix: Some("prettier --write ."),
    },
    Tool {
        name: "Biome",
        executable: "biome",
        configs: &["biome.json", "biome.jsonc"],
        check: "biome ci --formatter-enabled=true --linter-enabled=false --assist-enabled=false .",
        fix: Some("biome format --write ."),
    },
];
const LINTERS: &[Tool] = &[
    Tool {
        name: "Oxlint",
        executable: "oxlint",
        configs: &[
            ".oxlintrc.json",
            ".oxlintrc.jsonc",
            "oxlint.config.ts",
            "oxlint.config.mts",
            "oxlint.config.js",
        ],
        check: "oxlint .",
        fix: None,
    },
    Tool {
        name: "ESLint",
        executable: "eslint",
        configs: &[
            "eslint.config.js",
            "eslint.config.mjs",
            "eslint.config.cjs",
            "eslint.config.ts",
            "eslint.config.mts",
            "eslint.config.cts",
            ".eslintrc",
            ".eslintrc.json",
            ".eslintrc.js",
            ".eslintrc.cjs",
            ".eslintrc.yml",
            ".eslintrc.yaml",
        ],
        check: "eslint --no-fix .",
        fix: None,
    },
    Tool {
        name: "Biome",
        executable: "biome",
        configs: &["biome.json", "biome.jsonc"],
        check: "biome ci --linter-enabled=true --formatter-enabled=false --assist-enabled=false .",
        fix: None,
    },
];

struct Tool {
    name: &'static str,
    executable: &'static str,
    configs: &'static [&'static str],
    check: &'static str,
    fix: Option<&'static str>,
}

pub(crate) fn set_javascript_config_commands(
    root: &Path,
    orchestration: &mut OrchestrationConfig,
) -> Vec<String> {
    let mut missing = Vec::new();
    if orchestration.format_check.is_none()
        && orchestration.format.is_none()
        && let Some(tool) = select(root, FORMATTERS, "formatter", &mut missing)
    {
        orchestration.format_check = Some(tool.check.into());
        orchestration.format = tool.fix.map(str::to_owned);
    }
    if orchestration.lint.is_none()
        && let Some(tool) = select(root, LINTERS, "linter", &mut missing)
    {
        orchestration.lint = Some(tool.check.into());
    }
    missing
}

fn select<'a>(
    root: &Path,
    tools: &'a [Tool],
    role: &str,
    missing: &mut Vec<String>,
) -> Option<&'a Tool> {
    let package = fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());
    let mut configured: Vec<_> = tools
        .iter()
        .filter(|tool| has_any(root, tool.configs) || package_configuration(package.as_ref(), tool))
        .collect();
    if configured.is_empty() {
        configured = tools
            .iter()
            .filter(|tool| declared_dependency(package.as_ref(), tool))
            .collect();
    }
    let tool = match configured.as_slice() {
        [] => &tools[0],
        [tool] => *tool,
        _ => {
            missing.push(format!("{role}: multiple configurations detected ({}); set the corresponding [orchestration] command explicitly", configured.iter().map(|tool| tool.name).collect::<Vec<_>>().join(", ")));
            return None;
        }
    };
    if local_executable(root, tool.executable).is_none() {
        missing.push(format!("{role}: {} requires a repository-local executable; install declared dependencies or set the corresponding [orchestration] command explicitly", tool.name));
        return None;
    }
    Some(tool)
}

fn package_configuration(package: Option<&serde_json::Value>, tool: &Tool) -> bool {
    let key = match tool.executable {
        "prettier" => "prettier",
        "eslint" => "eslintConfig",
        _ => return false,
    };
    package.is_some_and(|value| value.get(key).is_some())
}

fn declared_dependency(package: Option<&serde_json::Value>, tool: &Tool) -> bool {
    let name = if tool.executable == "biome" {
        "@biomejs/biome"
    } else {
        tool.executable
    };
    package.is_some_and(|value| {
        ["dependencies", "devDependencies"].iter().any(|key| {
            value
                .get(key)
                .and_then(|dependencies| dependencies.get(name))
                .is_some()
        })
    })
}

/// Only canonical whole-project scripts can be safely converted. Preserve custom
/// paths/options by requiring an explicit command instead of guessing their scope.
pub(super) fn script_commands(
    command: &str,
    formatter: bool,
) -> Option<(&'static str, Option<&'static str>)> {
    let tokens = crate::engines::orchestration::shell_words_split(command);
    let executable = tokens.first()?;
    let tools = if formatter { FORMATTERS } else { LINTERS };
    let tool = tools.iter().find(|tool| executable == tool.executable)?;
    if tokens.iter().skip(1).any(|token| {
        !matches!(
            token.as_str(),
            "." | "--check" | "--write" | "--fix" | "--no-fix" | "format" | "lint" | "check" | "ci"
        )
    }) {
        return None;
    }
    Some((tool.check, tool.fix))
}

pub(super) fn local_executable(root: &Path, tool: &str) -> Option<()> {
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

#[cfg(test)]
#[path = "tooling_tests.rs"]
mod tests;
