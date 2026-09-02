//! Hardgate CLI entry point

use clap::{Parser, Subcommand};
use colored::*;

#[derive(Parser)]
#[command(name = "hardgate")]
#[command(about = "Deterministic quality gates, hard budgets, and anti-gaming verification harness for AI agents", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize hardgate.toml in the current repository
    Init {
        #[arg(short, long, default_value = "strict-agent")]
        preset: String,
    },
    /// Run fast deterministic static gate checks
    Check {
        /// Format output for AI agents (token-efficient markdown)
        #[arg(long)]
        format: Option<String>,
        /// Check only git-modified or staged files
        #[arg(short, long)]
        diff: bool,
    },
    /// Run complete verification including coverage and mutation
    Verify {
        #[arg(long)]
        coverage_report: Option<String>,
        #[arg(long)]
        mutation_report: Option<String>,
    },
    /// Launch as a Model Context Protocol (MCP) server
    Mcp,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { preset } => {
            println!("{} Initializing Hardgate with preset [{}]...", "✓".green(), preset.bold());
        }
        Commands::Check { format, diff } => {
            let mode = if diff { "git diff" } else { "full workspace" };
            println!("{} Running Hardgate check on {}...", "🛡️".blue(), mode);
            if format.as_deref() == Some("agent") {
                println!("Output mode: Agent (Token-Optimized)");
            }
        }
        Commands::Verify { .. } => {
            println!("{} Running Hardgate full verification...", "🛡️".blue());
        }
        Commands::Mcp => {
            println!("Starting Hardgate MCP server on stdio...");
        }
    }
}
