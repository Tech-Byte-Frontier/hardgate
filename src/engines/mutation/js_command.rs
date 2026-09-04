use super::js::{PackageManager, TestFramework};
use std::path::Path;
use std::str::Chars;

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
        let base = manager_script_command(input.manager, script);
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
    let tokens = shell_tokens(command);
    if tokens.is_empty()
        || tokens
            .iter()
            .any(|token| matches!(token.as_str(), "&&" | "||" | ";" | "|"))
    {
        return None;
    }
    framework_for_executable(command_executable(&tokens)?)
}

pub(crate) fn is_exact_bun_test_command(command: &str) -> bool {
    let tokens = shell_tokens(command);
    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "&&" | "||" | ";" | "|"))
    {
        return false;
    }
    let Some(index) = first_non_assignment(&tokens) else {
        return false;
    };
    executable_name(&tokens[index]) == Some("bun")
        && tokens.len() == index + 2
        && tokens.get(index + 1).is_some_and(|token| token == "test")
}

fn shell_tokens(command: &str) -> Vec<String> {
    let mut parser = ShellTokenizer::default();
    let mut chars = command.chars().peekable();
    while let Some(character) = chars.next() {
        if parser.is_comment_start(character) {
            break;
        }
        parser.consume(character, &mut chars);
    }
    parser.finish()
}

#[derive(Default)]
struct ShellTokenizer {
    tokens: Vec<String>,
    current: String,
    quote: Option<char>,
}

impl ShellTokenizer {
    fn is_comment_start(&self, character: char) -> bool {
        character == '#' && self.quote.is_none() && self.current.is_empty()
    }

    fn consume(&mut self, character: char, chars: &mut std::iter::Peekable<Chars<'_>>) {
        if let Some(active_quote) = self.quote {
            consume_quoted_character(character, active_quote, &mut self.quote, &mut self.current);
            return;
        }
        match character {
            '\'' | '"' => self.quote = Some(character),
            character if character.is_whitespace() => {
                push_token(&mut self.tokens, &mut self.current)
            }
            '&' if chars.peek() == Some(&'&') => {
                push_double_separator(chars, &mut self.tokens, &mut self.current, "&&")
            }
            '|' if chars.peek() == Some(&'|') => {
                push_double_separator(chars, &mut self.tokens, &mut self.current, "||")
            }
            ';' | '|' => push_single_separator(&mut self.tokens, &mut self.current, character),
            _ => self.current.push(character),
        }
    }

    fn finish(mut self) -> Vec<String> {
        push_token(&mut self.tokens, &mut self.current);
        self.tokens
    }
}

fn consume_quoted_character(
    character: char,
    active_quote: char,
    quote: &mut Option<char>,
    current: &mut String,
) {
    if character == active_quote {
        *quote = None;
    } else {
        current.push(character);
    }
}

fn push_double_separator(
    chars: &mut std::iter::Peekable<Chars<'_>>,
    tokens: &mut Vec<String>,
    current: &mut String,
    separator: &str,
) {
    chars.next();
    push_token(tokens, current);
    tokens.push(separator.to_string());
}

fn push_single_separator(tokens: &mut Vec<String>, current: &mut String, separator: char) {
    push_token(tokens, current);
    tokens.push(separator.to_string());
}

fn push_token(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

fn command_executable(tokens: &[String]) -> Option<&str> {
    let index = first_non_assignment(tokens)?;
    let executable = executable_name(&tokens[index])?;
    command_after_wrapper(executable, &tokens[index + 1..])
}

fn command_after_wrapper<'a>(executable: &'a str, args: &'a [String]) -> Option<&'a str> {
    match executable {
        "npx" | "bunx" => first_non_option(args),
        "pnpm" | "yarn" | "npm" | "bun" => package_manager_command(executable, args),
        "env" | "cross-env" => args
            .iter()
            .find(|token| !is_environment_assignment(token) && !token.starts_with('-'))
            .and_then(|token| executable_name(token)),
        _ => Some(executable),
    }
}

fn package_manager_command<'a>(manager: &str, args: &'a [String]) -> Option<&'a str> {
    if manager == "yarn" {
        if let Some(subcommand) = args.first().and_then(|token| executable_name(token))
            && framework_for_executable(subcommand).is_some()
        {
            return Some(subcommand);
        }
    }
    let skip = package_manager_exec_skip(manager, args.first()?.as_str())?;
    first_non_option(args.get(skip..)?)
}

fn package_manager_exec_skip(manager: &str, subcommand: &str) -> Option<usize> {
    match (manager, subcommand) {
        ("pnpm", "exec" | "dlx") | ("yarn", "exec") | ("npm", "exec" | "x") | ("bun", "x") => {
            Some(1)
        }
        _ => None,
    }
}

fn first_non_option(tokens: &[String]) -> Option<&str> {
    tokens
        .iter()
        .find(|token| !token.starts_with('-'))
        .and_then(|token| executable_name(token))
}

fn first_non_assignment(tokens: &[String]) -> Option<usize> {
    tokens
        .iter()
        .position(|token| !is_environment_assignment(token))
}

fn is_environment_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn executable_name(token: &str) -> Option<&str> {
    let token = token.rsplit(['/', '\\']).next()?.trim_end_matches(".cmd");
    (!token.is_empty()).then_some(token)
}

fn framework_for_executable(executable: &str) -> Option<TestFramework> {
    match executable {
        "jest" => Some(TestFramework::Jest),
        "vitest" => Some(TestFramework::Vitest),
        "playwright" => Some(TestFramework::Playwright),
        _ => None,
    }
}
