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
    let engine = OrchestrationEngine::new(&context.config.orchestration);
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
