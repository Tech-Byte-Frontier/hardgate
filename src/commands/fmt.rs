use crate::config::HardgateConfig;
use crate::engines::OrchestrationEngine;
use anyhow::Result;
use colored::*;
use std::path::Path;

/// Format the project with the configured `[orchestration]` formatter.
/// With `check_only`, verify formatting without writing changes.
pub fn cmd_fmt(check_only: bool) -> Result<()> {
    let config = HardgateConfig::load_or_default(None)?;
    let engine = OrchestrationEngine::new(&config.orchestration);
    let root = Path::new(".");

    let res = if check_only {
        engine.run_format_check(root)
    } else {
        engine.run_format(root)
    };

    let Some(res) = res else {
        println!(
            "{} no format or format_check command configured in [orchestration].",
            "warning:".yellow().bold()
        );
        return Ok(());
    };

    match res {
        Ok(ok) => {
            println!(
                "{} format [{}] passed ({}ms)",
                "ok:".green().bold(),
                ok.command.bold(),
                ok.duration_ms
            );
            if !ok.output.is_empty() {
                println!("{}", ok.output.dimmed());
            }
            Ok(())
        }
        Err(err) => {
            eprintln!(
                "{} format [{}] failed (exit: {:?})",
                "error:".red().bold(),
                err.command.bold(),
                err.exit_code
            );
            if !err.output.is_empty() {
                eprintln!("{}", err.output);
            }
            std::process::exit(1);
        }
    }
}
