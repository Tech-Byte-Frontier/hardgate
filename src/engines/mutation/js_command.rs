use super::js::{PackageManager, TestFramework};
use std::path::Path;

pub(crate) struct JsCommandInput<'a> {
    pub(crate) manager: PackageManager,
    pub(crate) framework: Option<TestFramework>,
    pub(crate) script: Option<&'a str>,
    pub(crate) candidate: Option<&'a Path>,
    pub(crate) working_dir: &'a Path,
}

pub(crate) fn build_js_command(input: JsCommandInput<'_>) -> String {
    if let Some(script) = input.script {
        let base = manager_script_command(input.manager, script);
        return if input.framework.is_some() {
            append_candidate(base, input.candidate, input.working_dir)
        } else {
            base
        };
    }
    if let Some(framework) = input.framework {
        let mut command = manager_exec_command(input.manager, framework);
        if !framework.args().is_empty() {
            command.push(' ');
            command.push_str(framework.args());
        }
        return append_candidate(command, input.candidate, input.working_dir);
    }
    manager_full_suite_command(input.manager)
}

fn manager_script_command(manager: PackageManager, script: &str) -> String {
    if script == "test" {
        return manager_full_suite_command(manager);
    }
    match manager {
        PackageManager::Npm => format!("npm run {script}"),
        PackageManager::Pnpm => format!("pnpm run {script}"),
        PackageManager::Yarn => format!("yarn {script}"),
        PackageManager::Bun => format!("bun run {script}"),
    }
}

fn manager_exec_command(manager: PackageManager, framework: TestFramework) -> String {
    match manager {
        PackageManager::Npm => format!("npm exec --offline -- {}", framework.binary()),
        PackageManager::Pnpm => format!("pnpm exec {}", framework.binary()),
        PackageManager::Yarn => format!("yarn exec {}", framework.binary()),
        PackageManager::Bun => format!("bun x --no-install {}", framework.binary()),
    }
}

fn manager_full_suite_command(manager: PackageManager) -> String {
    match manager {
        PackageManager::Npm => "npm test".to_string(),
        PackageManager::Pnpm => "pnpm test".to_string(),
        PackageManager::Yarn => "yarn test".to_string(),
        PackageManager::Bun => "bun test".to_string(),
    }
}

fn append_candidate(base: String, candidate: Option<&Path>, working_dir: &Path) -> String {
    let Some(candidate) = candidate else {
        return base;
    };
    let relative = candidate
        .strip_prefix(working_dir)
        .unwrap_or(candidate)
        .to_string_lossy();
    format!("{base} -- {}", shell_quote(&relative))
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._/-".contains(character))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}
