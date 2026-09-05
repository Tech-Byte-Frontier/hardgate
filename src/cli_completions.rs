use clap::CommandFactory;
use hardgate::commands::{CommandOutcome, CommandResult};
use std::io::{self, Write};

pub(super) fn generate(shell: clap_complete::Shell) -> CommandResult {
    // clap_complete writes infallibly internally; buffer before touching stdout.
    let mut buffer = Vec::new();
    clap_complete::generate(shell, &mut super::Cli::command(), "hardgate", &mut buffer);
    let mut out = io::BufWriter::new(io::stdout().lock());
    out.write_all(&buffer)?;
    out.flush()?;
    Ok(CommandOutcome::Passed)
}
