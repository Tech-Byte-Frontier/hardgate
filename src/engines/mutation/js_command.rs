use super::js::{PackageManager, TestFramework};
use std::path::Path;
pub(crate) struct JsCommandInput<'a> {
    pub(crate) manager: PackageManager,
    pub(crate) framework: Option<TestFramework>,
    pub(crate) script: Option<&'a str>,
    pub(crate) candidate: Option<&'a Path>,
    pub(crate) selector_capable: bool,
    pub(crate) bun_test_script: bool,
    pub(crate) working_dir: &'a Path,
}
pub(crate) fn build_js_command(input: JsCommandInput<'_>) -> String {
    if let Some(script) = input.script {
        let base = if input.bun_test_script {
            manager_full_suite_command(input.manager)
        } else {
            manager_script_command(input.manager, script)
        };
        return if input.selector_capable && input.bun_test_script {
            append_bun_candidate(base, input.candidate, input.working_dir)
        } else if input.selector_capable {
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
    if script == "test" && manager != PackageManager::Bun {
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
fn append_bun_candidate(base: String, candidate: Option<&Path>, working_dir: &Path) -> String {
    let Some(candidate) = candidate else {
        return base;
    };
    let relative = candidate
        .strip_prefix(working_dir)
        .unwrap_or(candidate)
        .to_string_lossy();
    format!("{base} {}", shell_quote(&relative))
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
pub(crate) fn framework_from_command(command: &str) -> Option<TestFramework> {
    let tokens = shell_tokens(command)?;
    if tokens.is_empty() || tokens.iter().any(|token| token.contains('=')) {
        return None;
    }
    framework_for_token(command_executable(&tokens)?)
}
pub(crate) fn is_exact_bun_test_command(command: &str) -> bool {
    let Some(tokens) = shell_tokens(command) else {
        return false;
    };
    tokens.len() == 2 && tokens[0] == "bun" && tokens[1] == "test"
}
fn shell_tokens(command: &str) -> Option<Vec<String>> {
    if command.chars().any(|character| {
        matches!(
            character,
            '\n' | '\r'
                | '#'
                | '&'
                | ';'
                | '|'
                | '<'
                | '>'
                | '$'
                | '('
                | ')'
                | '`'
                | '\''
                | '"'
                | '\\'
        )
    }) {
        return None;
    }
    Some(command.split_whitespace().map(str::to_string).collect())
}
fn command_executable(tokens: &[String]) -> Option<&str> {
    let token = tokens.first()?;
    let executable = executable_name(token)?;
    command_after_wrapper(token, executable, &tokens[1..])
}
fn command_after_wrapper<'a>(
    token: &'a str,
    executable: &str,
    args: &'a [String],
) -> Option<&'a str> {
    match executable {
        "npx" | "bunx" if token == executable => first_executable_after_options(args),
        "pnpm" | "yarn" | "npm" | "bun" if token == executable => {
            package_manager_command(executable, args)
        }
        _ => Some(token),
    }
}
fn package_manager_command<'a>(manager: &str, args: &'a [String]) -> Option<&'a str> {
    let skip = package_manager_exec_skip(manager, args.first()?.as_str())?;
    first_executable_after_options(args.get(skip..)?)
}
fn package_manager_exec_skip(manager: &str, subcommand: &str) -> Option<usize> {
    match (manager, subcommand) {
        ("pnpm", "exec" | "dlx") | ("yarn", "exec") | ("npm", "exec" | "x") | ("bun", "x") => {
            Some(1)
        }
        _ => None,
    }
}
fn first_executable_after_options(tokens: &[String]) -> Option<&str> {
    let mut skip_value = false;
    for (index, token) in tokens.iter().enumerate() {
        if skip_value {
            if token == "--" || token.starts_with('-') {
                return None;
            }
            skip_value = false;
            continue;
        }
        if token == "--" {
            return tokens.get(index + 1).map(String::as_str);
        }
        if token.starts_with('-') {
            skip_value = option_requires_value(token)?;
            continue;
        }
        return Some(token);
    }
    None
}
fn option_requires_value(token: &str) -> Option<bool> {
    if token.contains('=') {
        return None;
    }
    let option = token;
    let requires_value = matches!(
        option,
        "-c" | "-p"
            | "-w"
            | "--call"
            | "--config"
            | "--cwd"
            | "--filter"
            | "--node-options"
            | "--package"
            | "--prefix"
            | "--workspace"
    );
    let allowed_flag = matches!(
        option,
        "--ignore-existing"
            | "--no-install"
            | "--offline"
            | "--prefer-offline"
            | "--prefer-online"
            | "--quiet"
            | "--yes"
    );
    (requires_value || allowed_flag).then_some(requires_value)
}
fn executable_name(token: &str) -> Option<&str> {
    let token = token.rsplit(['/', '\\']).next()?.trim_end_matches(".cmd");
    (!token.is_empty()).then_some(token)
}
fn framework_for_token(token: &str) -> Option<TestFramework> {
    let executable = executable_name(token)?;
    let normalized = token.replace('\\', "/");
    let normalized = normalized.strip_suffix(".cmd").unwrap_or(&normalized);
    let bin_path = format!("node_modules/.bin/{executable}");
    if normalized != executable && normalized != format!("./{bin_path}") && normalized != bin_path {
        return None;
    }
    framework_for_executable(executable)
}
fn framework_for_executable(executable: &str) -> Option<TestFramework> {
    match executable {
        "jest" => Some(TestFramework::Jest),
        "vitest" => Some(TestFramework::Vitest),
        "playwright" => Some(TestFramework::Playwright),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::mutation::js::{
        PackageManager, ResolvedTestPlan, TestFramework, TestSelection, resolve_js_test_plan,
    };
    use crate::engines::mutation::js_tests::test_support::{temp_root, write};
    use std::path::Path;
    fn plan(root: &Path, source: &str) -> ResolvedTestPlan {
        resolve_js_test_plan(&root.join(source), root).unwrap()
    }
    struct CommandCase<'a> {
        manager: PackageManager,
        framework: Option<TestFramework>,
        script: Option<&'a str>,
        selector_capable: bool,
        bun_test_script: bool,
    }
    fn command(case: CommandCase<'_>) -> String {
        build_js_command(JsCommandInput {
            manager: case.manager,
            framework: case.framework,
            script: case.script,
            candidate: Some(Path::new("tests/value.test.ts")),
            selector_capable: case.selector_capable,
            bun_test_script: case.bun_test_script,
            working_dir: Path::new("."),
        })
    }
    fn assert_full_suite(plan: &ResolvedTestPlan, label: &str) {
        assert_eq!(plan.framework, None, "{label}");
        assert_eq!(plan.selection, TestSelection::FullSuite, "{label}");
        assert!(!plan.command.contains("value.test.ts"), "{label}");
    }
    fn workspace_plan(root: &Path, manager: &str, script: Option<&str>) -> ResolvedTestPlan {
        let package = script.map_or_else(
            || format!(r#"{{"packageManager":"{manager}","workspaces":["packages/*"]}}"#),
            |script| format!(r#"{{"packageManager":"{manager}","workspaces":["packages/*"],"scripts":{{"test":"{script}"}}}}"#),
        );
        write(root, "package.json", &package);
        write(root, "packages/app/package.json", r#"{"name":"app"}"#);
        write(
            root,
            "packages/app/src/value.ts",
            "export const value = true;\n",
        );
        write(
            root,
            "packages/app/tests/value.test.ts",
            "test('value', () => {});\n",
        );
        plan(root, "packages/app/src/value.ts")
    }
    #[test]
    fn bun_script_commands_preserve_script_body_and_builtin_selector() {
        assert_eq!(
            command(CommandCase {
                manager: Bun,
                framework: Some(Vitest),
                script: Some("test"),
                selector_capable: true,
                bun_test_script: false,
            }),
            "bun run test -- tests/value.test.ts"
        );
        assert_eq!(
            command(CommandCase {
                manager: Bun,
                framework: None,
                script: Some("test"),
                selector_capable: false,
                bun_test_script: false,
            }),
            "bun run test"
        );
        assert_eq!(
            command(CommandCase {
                manager: Bun,
                framework: None,
                script: Some("test"),
                selector_capable: true,
                bun_test_script: true,
            }),
            "bun test tests/value.test.ts"
        );
        assert_eq!(
            command(CommandCase {
                manager: Bun,
                framework: None,
                script: Some("test:unit"),
                selector_capable: false,
                bun_test_script: false,
            }),
            "bun run test:unit"
        );
        assert!(is_exact_bun_test_command("bun test"));
        assert!(!is_exact_bun_test_command("bun test # comment"));
        assert_eq!(framework_from_command("jest"), Some(TestFramework::Jest));
        assert_eq!(
            framework_from_command("vitest run"),
            Some(TestFramework::Vitest)
        );
        assert_eq!(
            framework_from_command("playwright test"),
            Some(TestFramework::Playwright)
        );
    }
    #[test]
    fn shell_composition_comments_and_unclosed_tokens_disable_selectors() {
        for command in [
            "jest # comment",
            "jest\njest",
            "jest\n",
            "jest & echo done",
            "jest&echo done",
            "npx 'jest",
            "jest \\",
            "bun test # comment",
            "bun test\n",
        ] {
            assert_eq!(framework_from_command(command), None, "{command:?}");
            assert!(!is_exact_bun_test_command(command), "{command:?}");
        }
    }
    #[test]
    fn wrapper_option_values_and_helper_paths_are_not_frameworks() {
        for command in [
            "npx --package jest helper",
            "npx --package=jest helper",
            "npx --package -- jest",
            "npx --unknown jest",
            "npm exec --package jest node",
            "npm exec --package -- jest",
            "bunx --package vitest helper",
            "yarn jest",
            "FOO=bar bun test",
            "/opt/bun test",
            "scripts/npm exec jest",
            "/opt/npm exec jest",
            "./scripts/jest",
            "scripts/vitest",
            "/opt/playwright",
        ] {
            assert_eq!(framework_from_command(command), None, "{command}");
        }
        assert_eq!(
            framework_from_command("npx --package jest -- jest"),
            Some(TestFramework::Jest)
        );
        assert_eq!(
            framework_from_command("yarn exec jest"),
            Some(TestFramework::Jest)
        );
        assert_eq!(
            framework_from_command("./node_modules/.bin/jest"),
            Some(TestFramework::Jest)
        );
    }
    #[test]
    fn workspace_root_script_is_bounded_fallback() {
        for (label, manager, expected) in [
            ("npm", "npm@10", "npm test"),
            ("pnpm", "pnpm@9", "pnpm test"),
            ("yarn", "yarn@4", "yarn test"),
            ("bun", "bun@1", "bun run test"),
        ] {
            let root = temp_root(label);
            let value = workspace_plan(&root, manager, Some("node scripts/test.mjs"));
            assert_eq!(value.command, expected, "{label}");
            assert_eq!(value.working_dir, root, "{label}");
            assert_full_suite(&value, label);
            let _ = std::fs::remove_dir_all(root);
        }
        let root = temp_root("workspace-no-root-script");
        let value = workspace_plan(&root, "npm@10", None);
        assert_eq!(value.command, "npm test");
        assert_eq!(value.working_dir, root.join("packages/app"));
        assert_full_suite(&value, "no root script");
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn nested_package_and_symlinked_tests_do_not_leak_candidates() {
        let root = temp_root("nested-package-tests");
        write(
            &root,
            "package.json",
            r#"{"packageManager":"npm@10","scripts":{"test":"jest"}}"#,
        );
        write(&root, "src/value.ts", "export const value = true;\n");
        write(&root, "tests/package.json", r#"{"name":"nested-tests"}"#);
        write(&root, "tests/value.test.ts", "test('value', () => {});\n");
        let value = plan(&root, "src/value.ts");
        assert_full_suite(&value, "nested package");
        let _ = std::fs::remove_dir_all(&root);
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let root = temp_root("symlinked-tests");
            let outside = temp_root("symlink-target");
            write(
                &root,
                "package.json",
                r#"{"packageManager":"npm@10","scripts":{"test":"jest"}}"#,
            );
            write(&root, "src/value.ts", "export const value = true;\n");
            write(&outside, "value.test.ts", "test('value', () => {});\n");
            std::fs::create_dir_all(root.join("tests")).unwrap();
            symlink(&outside, root.join("tests/outside")).unwrap();
            let value = plan(&root, "src/value.ts");
            assert_full_suite(&value, "symlink outside package");
            let _ = std::fs::remove_dir_all(root);
            let _ = std::fs::remove_dir_all(outside);
        }
    }
}
