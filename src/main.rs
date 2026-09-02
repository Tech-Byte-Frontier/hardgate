use clap::{Parser, Subcommand};
use hardgate::commands;
use hardgate::mcp;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "hardgate")]
#[command(
    about = "Deterministic quality gates, hard budgets, and anti-gaming verification harness for the AI agent era",
    long_about = None
)]
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
        /// Format output (agent | json)
        #[arg(long)]
        format: Option<String>,
        /// Check only git-modified or staged files
        #[arg(short, long)]
        diff: bool,
    },
    /// Immediately inspect AST metrics, suppressions, and budgets for a single file
    Scan {
        /// File path to scan
        file: PathBuf,
        /// Format output (agent | json)
        #[arg(long)]
        format: Option<String>,
    },
    /// Run complete verification including coverage and mutation
    Verify {
        /// Path to coverage report (e.g., coverage/lcov.info)
        #[arg(long)]
        coverage_report: Option<String>,
        /// Path to mutation report (e.g., mutants.json or stryker-mutation.json)
        #[arg(long)]
        mutation_report: Option<String>,
        /// Format output (agent | json)
        #[arg(long)]
        format: Option<String>,
    },
    /// Launch as a Model Context Protocol (MCP) server over stdio
    Mcp,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    execute_command(cli.command)
}

fn execute_command(cmd: Commands) -> anyhow::Result<()> {
    match cmd {
        Commands::Init { preset } => commands::cmd_init(&preset),
        Commands::Check { format, diff } => commands::cmd_check(format.as_deref(), diff),
        Commands::Scan { file, format } => commands::cmd_scan(&file, format.as_deref()),
        Commands::Verify {
            coverage_report,
            mutation_report,
            format,
        } => commands::cmd_verify(coverage_report, mutation_report, format.as_deref()),
        Commands::Mcp => mcp::run_mcp_server(),
    }
}
