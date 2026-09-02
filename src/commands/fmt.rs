use crate::config::HardgateConfig;
use crate::engines::OrchestrationEngine;
use anyhow::Result;
use colored::*;
use std::path::Path;

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
            "{} No format or format_check command configured in [orchestration].",
            "⚠️".yellow()
        );
        return Ok(());
    };

    match res {
        Ok(ok) => {
            println!(
                "{} Format [{}] passed ({}ms)",
                "✓".green(),
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
                "{} Format [{}] failed (exit code: {:?})",
                "❌".red(),
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
