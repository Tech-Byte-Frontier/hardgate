use super::{Cli, Commands, Parser, run_cli};
use hardgate::commands::{CommandOutcome, CommandResult, MutationFailure, outcome::is_broken_pipe};
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;
use std::time::Instant;

pub(super) fn main_exit() -> ExitCode {
    let args = std::env::args_os().collect::<Vec<_>>();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => return parse_failure(error, wants_json(&args)),
    };
    configure_color(cli.color);
    let stage = command_stage(&cli.command);
    let json = command_json(&cli.command);
    let timing = cli.timing;
    let start = Instant::now();
    let result = run_cli(cli);
    if timing {
        let _ = writeln!(
            io::stderr().lock(),
            "hardgate: {stage} took {}ms",
            start.elapsed().as_millis()
        );
    }
    if let Some(signal) = hardgate::cancellation::signal() {
        return ExitCode::from((128 + signal) as u8);
    }
    finish(result, stage, json)
}

fn finish(result: CommandResult, stage: &str, json: bool) -> ExitCode {
    match result {
        Ok(outcome) => ExitCode::from(outcome.exit_code()),
        Err(error) if is_broken_pipe(&error) => ExitCode::SUCCESS,
        Err(error) => {
            let rendered = if json {
                emit_error(stage, &error)
            } else {
                writeln!(io::stderr().lock(), "hardgate: {error:#}")
            };
            if rendered.is_err_and(|error| error.kind() == io::ErrorKind::BrokenPipe) && json {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
    }
}

fn parse_failure(error: clap::Error, json: bool) -> ExitCode {
    if json && error.use_stderr() {
        return finish(Err(anyhow::anyhow!(error.to_string())), "arguments", true);
    }
    let status = if error.use_stderr() { 2 } else { 0 };
    match error.print() {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        _ => ExitCode::from(status),
    }
}

fn emit_error(stage: &str, error: &anyhow::Error) -> io::Result<()> {
    let mutation = error.downcast_ref::<MutationFailure>();
    let value = serde_json::json!({
        "schema_version": 1,
        "command": stage,
        "passed": false,
        "status": "error",
        "exit_code": CommandOutcome::Incomplete.exit_code(),
        "execution": error.downcast_ref::<hardgate::commands::ExecutionFailure>().map(|failure| &failure.plan),
        "stage": mutation.map_or(stage, |error| error.stage),
        "kind": mutation.map_or("command-error", |error| error.kind),
        "message": format!("{error:#}"),
    });
    let mut out = io::BufWriter::new(io::stdout().lock());
    serde_json::to_writer_pretty(&mut out, &value)?;
    writeln!(out)?;
    out.flush()
}

fn wants_json(args: &[std::ffi::OsString]) -> bool {
    args.iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
        || args
            .windows(2)
            .any(|pair| pair[0] == "--format" && pair[1] == "json")
}

fn command_json(command: &Commands) -> bool {
    match command {
        Commands::Check { output, .. }
        | Commands::Verify { output, .. }
        | Commands::Scan { output, .. } => output.output_options().is_json(),
        Commands::Mutate { format, json, .. } => *json || format.as_deref() == Some("json"),
        Commands::Config { format } => format == "json",
        _ => false,
    }
}

fn command_stage(command: &Commands) -> &'static str {
    match command {
        Commands::Check { .. } => "check",
        Commands::Scan { .. } => "scan",
        Commands::Mutate { .. } => "mutate",
        Commands::Verify { .. } => "verify",
        Commands::Config { .. } => "config",
        _ => utility_stage(command),
    }
}

fn utility_stage(command: &Commands) -> &'static str {
    match command {
        Commands::Init { .. } => "init",
        Commands::Completions { .. } => "completions",
        Commands::Fmt { .. } => "fmt",
        _ => "mcp",
    }
}

fn configure_color(choice: clap::ColorChoice) {
    let enabled = match choice {
        clap::ColorChoice::Always => true,
        clap::ColorChoice::Never => false,
        clap::ColorChoice::Auto => auto_color(),
    };
    colored::control::set_override(enabled);
}

fn auto_color() -> bool {
    let value = |name| std::env::var(name).ok().filter(|value| !value.is_empty());
    if value("NO_COLOR").is_some() {
        false
    } else if value("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
        true
    } else {
        io::stdout().is_terminal()
            && value("CLICOLOR").as_deref() != Some("0")
            && value("TERM").as_deref() != Some("dumb")
    }
}
