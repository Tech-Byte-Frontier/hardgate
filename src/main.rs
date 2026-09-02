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
        /// Run full orchestration (format check + linter) alongside static gates
        #[arg(short, long)]
        all: bool,
        /// Run dead code and unused export analysis
        #[arg(long)]
        dead_code: bool,
        /// Path to coverage report to verify against AST budgets
        #[arg(long)]
        coverage_report: Option<String>,
    },
    /// Immediately inspect AST metrics, suppressions, and budgets for a single file
    Scan {
        /// File path to scan
        file: PathBuf,
        /// Format output (agent | json)
        #[arg(long)]
        format: Option<String>,
    },
    /// Format code using orchestrated project formatter (e.g. oxfmt)
    Fmt {
        /// Check only without writing changes to disk
        #[arg(long)]
        check: bool,
    },
    /// Run native AST mutation testing against test runner
    Mutate {
        /// Mutate only git-modified files
        #[arg(short, long)]
        diff: bool,
        /// Scoped file or directory path to mutate
        #[arg(short, long)]
        scoped: Option<PathBuf>,
        /// Custom test command (e.g. "cargo test {stem}" or "pnpm test {file}")
        #[arg(long)]
        test_cmd: Option<String>,
        /// Timeout in seconds per mutant
        #[arg(long)]
        timeout: Option<u64>,
        /// Maximum number of mutants to evaluate
        #[arg(long)]
        max_mutants: Option<usize>,
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
        Commands::Check {
            format,
            diff,
            all,
            dead_code,
            coverage_report,
        } => commands::cmd_check(commands::CheckOptions {
            format,
            diff,
            all,
            dead_code,
            coverage_report,
        }),
        Commands::Scan { file, format } => commands::cmd_scan(&file, format.as_deref()),
        Commands::Fmt { check } => commands::cmd_fmt(check),
        Commands::Mutate {
            diff,
            scoped,
            test_cmd,
            timeout,
            max_mutants,
            format,
        } => commands::cmd_mutate(commands::MutateOptions {
            diff,
            scoped,
            test_cmd,
            timeout_secs: timeout,
            max_mutants,
            format,
        }),
        Commands::Verify {
            coverage_report,
            mutation_report,
            format,
        } => commands::cmd_verify(coverage_report, mutation_report, format.as_deref()),
        Commands::Mcp => mcp::run_mcp_server(),
    }
}
