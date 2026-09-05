use clap::{Args, Parser, Subcommand};
use hardgate::commands;
use hardgate::mcp;
use std::path::PathBuf;

mod build_info;
mod cli_completions;
mod cli_report_args;
mod cli_runtime;

#[derive(Parser)]
#[command(name = "hardgate")]
#[command(
    about = "Deterministic quality gates, hard budgets, and anti-gaming verification harness for the AI agent era",
    long_about = None
)]
#[command(version = build_info::VERSION_DISPLAY)]
struct Cli {
    /// Explicit policy file; its directory is the configuration root
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Limit analysis worker threads (positive integer; defaults to Rayon settings)
    #[arg(long, global = true, value_name = "N")]
    threads: Option<std::num::NonZeroUsize>,
    /// Terminal colors; auto respects TTY, NO_COLOR and CLICOLOR conventions
    #[arg(long, global = true, default_value = "auto")]
    color: clap::ColorChoice,
    /// Print total command timing to stderr
    #[arg(long, global = true)]
    timing: bool,
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
    /// Print concise summary only (totals + top files; combine as '--json --summary' for compact machine-readable rollup)
    #[arg(long)]
    summary: bool,
    /// Include bounded excerpts captured during analysis
    #[arg(long, conflicts_with = "no_snippets")]
    snippets: bool,
    /// Display at most N diagnostics; complete analysis and verdict are unchanged
    #[arg(long, value_name = "N")]
    max_diagnostics: Option<usize>,
    /// Write output directly to a file atomically
    #[arg(long = "output", value_name = "PATH")]
    output_file: Option<PathBuf>,
}

impl OutputArgs {
    fn output_options(&self) -> commands::OutputOptions {
        commands::OutputOptions {
            format: self.format.clone(),
            json: self.json,
            compact: self.compact,
            no_snippets: self.no_snippets,
            summary: self.summary,
            display: self.display_options(),
            output_file: self.output_file.clone(),
            progress: None,
        }
    }
    fn display_options(&self) -> hardgate::diagnostics::display::DisplayOptions {
        hardgate::diagnostics::display::DisplayOptions {
            snippets: self.snippets,
            max_diagnostics: self.max_diagnostics,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Print shell completion script without loading policy or running tools
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Initialize hardgate.toml in the current repository
    Init {
        /// Config preset: balanced (structural adoption), strict-agent (required evidence),
        /// legacy-migration (reference ratchet), or custom (ordinary defaults)
        #[arg(
            short,
            long,
            default_value = "balanced",
            value_parser = ["strict-agent", "balanced", "legacy-migration", "custom"],
            ignore_case = true
        )]
        preset: String,
        /// Print proposed TOML without writing a file
        #[arg(long)]
        preview: bool,
        /// Emit the full effective policy instead of preset plus overrides
        #[arg(long)]
        full: bool,
        /// Explicit command that fails when formatting changes are needed
        #[arg(long)]
        format_check: Option<String>,
        /// Explicit formatter command (may change project files when fmt runs)
        #[arg(long)]
        format_command: Option<String>,
        /// Explicit linter command
        #[arg(long)]
        lint: Option<String>,
    },
    /// Run fast deterministic static gate checks
    Check {
        #[command(flatten)]
        output: OutputArgs,
        /// Stream check progress events to stderr
        #[arg(long, value_parser = ["jsonl"])]
        progress: Option<String>,
        /// Check only git-modified or staged files
        #[arg(short, long)]
        diff: bool,
        /// Run configured format-check, linter, and test commands before verifying static gates, coverage, and mutation
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
        /// Format output (terminal | agent | json | summary)
        #[arg(long, value_parser = ["terminal", "agent", "json", "summary"])]
        format: Option<String>,
        /// Shorthand for --format json
        #[arg(long)]
        json: bool,
        /// Print concise summary only (totals, score, and survivors)
        #[arg(long)]
        summary: bool,
        /// Write output directly to a file atomically
        #[arg(long = "output", value_name = "PATH")]
        output_file: Option<PathBuf>,
    },
    /// Inspect or compare saved gate reports without rescanning
    #[command(args_conflicts_with_subcommands = true)]
    Report {
        #[command(subcommand)]
        subcommand: Option<ReportCommand>,
        /// Saved gate report JSON file to inspect
        #[arg(value_name = "FILE")]
        file: Option<PathBuf>,
        /// Filter findings to a specific engine
        #[arg(long)]
        engine: Option<String>,
        /// Filter findings by metric name
        #[arg(long)]
        metric: Option<String>,
        /// Show only top N files with most violations
        #[arg(long, value_name = "N")]
        top: Option<usize>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Evaluate static policy and verify configured coverage/mutation evidence reports without executing tools (report ingestion only)
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
    /// Show the fully merged and validated effective policy and its authority
    Config {
        #[arg(long, default_value = "toml", value_parser = ["toml", "json"])]
        format: String,
    },
    /// Launch as a Model Context Protocol (MCP) server over stdio
    Mcp,
}

#[derive(Subcommand)]
enum ReportCommand {
    /// Compare two saved gate reports and report differences in findings, scope, and policy
    Compare {
        /// Baseline / before gate report JSON
        before: PathBuf,
        /// Current / after gate report JSON
        after: PathBuf,
        #[command(flatten)]
        output: cli_report_args::CompareOutputArgs,
    },
}

fn main() -> std::process::ExitCode {
    cli_runtime::main_exit()
}

fn run_cli(cli: Cli) -> commands::CommandResult {
    let _ = build_info::identity();
    let _ = std::hint::black_box(build_info::TARGET);
    let _ = std::hint::black_box(build_info::BUILD_TARGET_MARKER);
    if matches!(cli.command, Commands::Mutate { .. }) {
        hardgate::cancellation::install()?;
    }
    if let Some(threads) = cli.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads.get())
            .build()?
            .install(|| execute_command(cli.command, cli.config.as_deref()))
    } else {
        execute_command(cli.command, cli.config.as_deref())
    }
}

fn execute_command(cmd: Commands, config: Option<&std::path::Path>) -> commands::CommandResult {
    match cmd {
        Commands::Completions { shell } => cli_completions::generate(shell),
        Commands::Init { .. } => execute_init_command(cmd, config),
        Commands::Report { .. } => execute_report_command(cmd),
        Commands::Mcp => {
            mcp::run_mcp_server_with_config(config).map(|()| commands::CommandOutcome::Passed)
        }
        command => {
            let context = hardgate::config::ConfigContext::load(config)?;
            execute_resolved_command(command, &context)
        }
    }
}

fn execute_init_command(
    cmd: Commands,
    config: Option<&std::path::Path>,
) -> commands::CommandResult {
    let Commands::Init {
        preset,
        preview,
        full,
        format_check,
        format_command,
        lint,
    } = cmd
    else {
        anyhow::bail!("expected init command");
    };
    anyhow::ensure!(
        config.is_none(),
        "init writes hardgate.toml in the current directory; --config selects an existing policy"
    );
    commands::init::cmd_init_with_options(commands::init::InitOptions {
        preset,
        preview,
        full,
        format_check,
        format: format_command,
        lint,
    })
    .map(|()| commands::CommandOutcome::Passed)
}

fn execute_report_command(cmd: Commands) -> commands::CommandResult {
    let Commands::Report {
        subcommand,
        file,
        engine,
        metric,
        top,
        output,
    } = cmd
    else {
        anyhow::bail!("expected report command");
    };
    match subcommand {
        Some(ReportCommand::Compare {
            before,
            after,
            output,
        }) => commands::cmd_report_compare(before, after, output.output_options()),
        None => {
            let Some(file) = file else {
                anyhow::bail!(
                    "Specify a report JSON file to inspect, or use `report compare <BEFORE> <AFTER>`"
                );
            };
            commands::cmd_report_inspect(commands::ReportInspectOptions {
                file,
                engine,
                metric,
                top,
                output: output.output_options(),
            })
        }
    }
}

fn execute_resolved_command(
    cmd: Commands,
    context: &hardgate::config::ConfigContext,
) -> commands::CommandResult {
    match cmd {
        Commands::Fmt { check } => commands::cmd_fmt_in(check, context),
        Commands::Config { format } => commands::inspect::cmd_config(context, &format)
            .map(|()| commands::CommandOutcome::Passed),
        gate => execute_gate_command(gate, context),
    }
}

fn execute_gate_command(
    cmd: Commands,
    context: &hardgate::config::ConfigContext,
) -> commands::CommandResult {
    match cmd {
        Commands::Check { .. } => execute_check_command(cmd, context),
        Commands::Scan { file, output } => {
            commands::cmd_scan_in(&file, output.output_options(), context)
        }
        Commands::Mutate { .. } => execute_mutate_command(cmd, context),
        Commands::Verify { .. } => execute_verify_command(cmd, context),
        _ => anyhow::bail!("internal command routing error"),
    }
}

fn execute_check_command(
    cmd: Commands,
    context: &hardgate::config::ConfigContext,
) -> commands::CommandResult {
    let Commands::Check {
        output,
        progress,
        diff,
        all,
        dead_code,
        coverage_report,
        paths,
    } = cmd
    else {
        anyhow::bail!("expected check command");
    };
    let opts = output.output_options();
    commands::cmd_check_in(
        commands::CheckOptions {
            format: opts.format,
            diff,
            all,
            dead_code,
            coverage_report,
            json: opts.json,
            compact: opts.compact,
            no_snippets: opts.no_snippets,
            summary: opts.summary,
            display: opts.display,
            paths,
            output_file: opts.output_file,
            progress,
        },
        context,
    )
}

fn execute_mutate_command(
    cmd: Commands,
    context: &hardgate::config::ConfigContext,
) -> commands::CommandResult {
    let Commands::Mutate {
        diff,
        scoped,
        test_cmd,
        timeout,
        max_mutants,
        format,
        json,
        summary,
        output_file,
    } = cmd
    else {
        anyhow::bail!("expected mutate command");
    };
    commands::cmd_mutate_in(
        commands::MutateOptions {
            diff,
            scoped,
            test_cmd,
            timeout_secs: timeout,
            max_mutants,
            format: resolve_mutate_format(format, json),
            summary,
            output_file,
        },
        context,
    )
}

fn execute_verify_command(
    cmd: Commands,
    context: &hardgate::config::ConfigContext,
) -> commands::CommandResult {
    let Commands::Verify {
        coverage_report,
        mutation_report,
        output,
        paths,
    } = cmd
    else {
        anyhow::bail!("expected verify command");
    };
    let opts = output.output_options();
    commands::cmd_verify_in(
        commands::VerifyOptions {
            coverage_report,
            mutation_report,
            format: opts.format,
            json: opts.json,
            compact: opts.compact,
            no_snippets: opts.no_snippets,
            summary: opts.summary,
            display: opts.display,
            paths,
            output_file: opts.output_file,
        },
        context,
    )
}

fn resolve_mutate_format(format: Option<String>, json: bool) -> Option<String> {
    if json {
        return Some("json".to_string());
    }
    format
}
