use clap::{Args, Parser, Subcommand};
use hardgate::commands;
use hardgate::mcp;
use std::path::PathBuf;

mod build_info;

#[derive(Parser)]
#[command(name = "hardgate")]
#[command(
    about = "Deterministic quality gates, hard budgets, and anti-gaming verification harness for the AI agent era",
    long_about = None
)]
#[command(version = build_info::VERSION_DISPLAY)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Shared machine-consumable output flags for gate commands.
///
/// `--json` is shorthand for `--format json`, `--compact`/`--no-snippets`
/// collapse each violation to one line, and `--summary` prints totals plus
/// top offending files.
#[derive(Args, Debug, Clone, Default)]
struct OutputArgs {
    /// Format output (terminal | agent | json | compact | summary)
    #[arg(long, value_parser = ["terminal", "agent", "json", "compact", "summary"])]
    format: Option<String>,
    /// Shorthand for --format json (machine-readable, jq-friendly)
    #[arg(long)]
    json: bool,
    /// Compact one-line-per-violation output without snippets or details
    #[arg(long)]
    compact: bool,
    /// Alias for --compact (no source snippets or breakdowns)
    #[arg(long = "no-snippets")]
    no_snippets: bool,
    /// Print concise summary only (totals + top files)
    #[arg(long)]
    summary: bool,
}

impl OutputArgs {
    fn output_options(&self) -> commands::OutputOptions {
        commands::OutputOptions {
            format: self.format.clone(),
            json: self.json,
            compact: self.compact,
            no_snippets: self.no_snippets,
            summary: self.summary,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize hardgate.toml in the current repository
    Init {
        /// Config preset: strict-agent (AI agents), balanced (hybrid teams),
        /// legacy-migration (burn down tech debt), or custom (empty shell)
        #[arg(
            short,
            long,
            default_value = "strict-agent",
            value_parser = ["strict-agent", "balanced", "legacy-migration", "custom"],
            ignore_case = true
        )]
        preset: String,
    },
    /// Run fast deterministic static gate checks
    Check {
        #[command(flatten)]
        output: OutputArgs,
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
        /// Optional path filter(s): only check files under these paths
        #[arg(value_name = "PATH")]
        paths: Vec<PathBuf>,
    },
    /// Immediately inspect AST metrics, suppressions, and budgets for a single file
    Scan {
        /// File path to scan
        file: PathBuf,
        #[command(flatten)]
        output: OutputArgs,
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
        /// Format output (terminal | agent | json)
        #[arg(long, value_parser = ["terminal", "agent", "json"])]
        format: Option<String>,
        /// Shorthand for --format json
        #[arg(long)]
        json: bool,
    },
    /// Run complete verification including coverage and mutation
    Verify {
        /// Path to coverage report (e.g., coverage/lcov.info)
        #[arg(long)]
        coverage_report: Option<String>,
        /// Path to mutation report (e.g., mutants.json or stryker-mutation.json)
        #[arg(long)]
        mutation_report: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
        /// Optional path filter(s): only verify files under these paths
        #[arg(value_name = "PATH")]
        paths: Vec<PathBuf>,
    },
    /// Launch as a Model Context Protocol (MCP) server over stdio
    Mcp,
}

fn main() -> anyhow::Result<()> {
    let _ = build_info::identity();
    let cli = Cli::parse();
    execute_command(cli.command)
}

fn execute_command(cmd: Commands) -> anyhow::Result<()> {
    match cmd {
        Commands::Init { preset } => commands::cmd_init(&preset),
        Commands::Fmt { check } => commands::cmd_fmt(check),
        Commands::Mcp => mcp::run_mcp_server(),
        gate => execute_gate_command(gate),
    }
}

fn execute_gate_command(cmd: Commands) -> anyhow::Result<()> {
    match cmd {
        Commands::Check {
            output,
            diff,
            all,
            dead_code,
            coverage_report,
            paths,
        } => {
            let opts = output.output_options();
            commands::cmd_check(commands::CheckOptions {
                format: opts.format,
                diff,
                all,
                dead_code,
                coverage_report,
                json: opts.json,
                compact: opts.compact,
                no_snippets: opts.no_snippets,
                summary: opts.summary,
                paths,
            })
        }
        Commands::Scan { file, output } => commands::cmd_scan(&file, output.output_options()),
        Commands::Mutate {
            diff,
            scoped,
            test_cmd,
            timeout,
            max_mutants,
            format,
            json,
        } => commands::cmd_mutate(commands::MutateOptions {
            diff,
            scoped,
            test_cmd,
            timeout_secs: timeout,
            max_mutants,
            format: resolve_mutate_format(format, json),
        }),
        Commands::Verify {
            coverage_report,
            mutation_report,
            output,
            paths,
        } => {
            let opts = output.output_options();
            commands::cmd_verify(commands::VerifyOptions {
                coverage_report,
                mutation_report,
                format: opts.format,
                json: opts.json,
                compact: opts.compact,
                no_snippets: opts.no_snippets,
                summary: opts.summary,
                paths,
            })
        }
        other => execute_command(other),
    }
}

fn resolve_mutate_format(format: Option<String>, json: bool) -> Option<String> {
    if json && format.is_none() {
        return Some("json".to_string());
    }
    format
}
