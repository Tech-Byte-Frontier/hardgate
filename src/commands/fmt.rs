use super::outcome::{CommandOutcome, CommandResult, write_stdout};
use crate::config::ConfigContext;
use crate::engines::OrchestrationEngine;
use colored::*;
use std::io::Write;

/// Run the explicitly configured formatter, optionally checking without writes.
pub fn cmd_fmt(check_only: bool) -> CommandResult {
    cmd_fmt_in(check_only, &ConfigContext::load(None)?)
}

pub fn cmd_fmt_in(check_only: bool, context: &ConfigContext) -> CommandResult {
    run_format(check_only, context, &context.config.orchestration)
}

fn run_format(
    check_only: bool,
    context: &ConfigContext,
    config: &crate::config::OrchestrationConfig,
) -> CommandResult {
    let engine = OrchestrationEngine::new(config);
    let result = if check_only {
        engine.run_format_check(&context.root)
    } else {
        engine.run_format(&context.root)
    };
    let result = result.ok_or_else(|| {
        anyhow::anyhow!("Configure [orchestration].format or format_check before running fmt")
    })?;
    match result {
        Ok(result) => {
            write_stdout(&format!(
                "{} format [{}] passed ({}ms)\n{}\n",
                "ok:".green().bold(),
                result.command,
                result.duration_ms,
                result.output
            ))?;
            Ok(CommandOutcome::Passed)
        }
        Err(failure) => {
            writeln!(
                std::io::stderr().lock(),
                "{} format [{}] failed (exit: {:?})\n{}",
                "error:".red().bold(),
                failure.command,
                failure.exit_code,
                failure.output
            )?;
            Ok(
                if failure.exit_code.is_none() || matches!(failure.exit_code, Some(126 | 127)) {
                    CommandOutcome::Incomplete
                } else {
                    CommandOutcome::Violations
                },
            )
        }
    }
}

pub fn cmd_fmt_scoped(
    check_only: bool,
    changed: bool,
    files: &[std::path::PathBuf],
    context: &ConfigContext,
) -> CommandResult {
    if !changed && files.is_empty() {
        return cmd_fmt_in(check_only, context);
    }
    let paths = selected_paths(changed, files, context)?;
    if paths.is_empty() {
        write_stdout("No changed files to format.\n")?;
        return Ok(CommandOutcome::Passed);
    }
    let mut config = context.config.orchestration.clone();
    let (key, template) = if check_only {
        ("format_check_files", &config.format_check_files)
    } else {
        ("format_files", &config.format_files)
    };
    let template = template.as_deref().ok_or_else(|| anyhow::anyhow!("Configure [orchestration].{key} with a standalone {{files}} token, e.g. 'oxfmt {}{{files}}'", if check_only { "--check " } else { "" }))?;
    let tokens = crate::engines::orchestration::shell_words_split(template);
    anyhow::ensure!(
        tokens
            .iter()
            .filter(|token| token.as_str() == "{files}")
            .count()
            == 1
            && tokens.first().is_some_and(|token| token != "{files}"),
        "{key} requires exactly one standalone {{files}} argument"
    );
    let expanded = tokens
        .iter()
        .flat_map(|token| {
            if token == "{files}" {
                paths.iter().cloned().collect()
            } else {
                vec![token.clone()]
            }
        })
        .map(|token| quote_token(&token))
        .collect::<Vec<_>>()
        .join(" ");
    if check_only {
        config.format_check = Some(expanded);
    } else {
        config.format = Some(expanded);
    }
    run_format(check_only, context, &config)
}

fn quote_token(token: &str) -> String {
    format!("'{}'", token.replace('\'', "'\\''"))
}

fn changed_files(root: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for args in [
        vec![
            "diff",
            "--name-only",
            "--diff-filter=ACMRT",
            "-z",
            "--relative",
            "--",
        ],
        vec![
            "diff",
            "--cached",
            "--relative",
            "--name-only",
            "--diff-filter=ACMRT",
            "-z",
            "--",
        ],
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
    ] {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "cannot select changed files: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for path in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            files.push(std::path::PathBuf::from(std::str::from_utf8(path)?));
        }
    }
    Ok(files)
}

fn selected_paths(
    changed: bool,
    files: &[std::path::PathBuf],
    context: &ConfigContext,
) -> anyhow::Result<std::collections::BTreeSet<String>> {
    let selected = if changed {
        changed_files(&context.root)?
    } else {
        files.to_vec()
    };
    let mut paths = std::collections::BTreeSet::new();
    let root = context.root.canonicalize()?;
    for file in selected {
        let path = if changed {
            root.join(&file)
        } else {
            std::env::current_dir()?.join(&file)
        };
        if changed && !path.exists() {
            continue;
        }
        let resolved = path.canonicalize()?;
        anyhow::ensure!(
            resolved.starts_with(&root) && resolved.is_file(),
            "format selection must be a file inside {}: {}",
            root.display(),
            file.display()
        );
        // Root-relative ./ prevents filenames beginning with '-' becoming options.
        paths.insert(format!(
            "./{}",
            resolved
                .strip_prefix(&root)?
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("formatter path is not UTF-8"))?
        ));
    }
    Ok(paths)
}
