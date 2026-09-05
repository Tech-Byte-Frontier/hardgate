use anyhow::Result;
use colored::*;
use std::io::Write;
use std::path::PathBuf;

pub(super) fn print_generation_notice(files: &[PathBuf], diff: bool) -> Result<()> {
    writeln!(
        std::io::stdout().lock(),
        "{} generating AST mutations across {} source files (diff: {})...",
        "note:".bold(),
        files.len().to_string().cyan(),
        diff
    )?;
    Ok(())
}

pub(super) fn print_mutant_notice(count: usize, timeout: u64) -> Result<()> {
    writeln!(
        std::io::stdout().lock(),
        "{} running {} mutants (timeout: {}s per mutant)...",
        "note:".bold(),
        count.to_string().cyan(),
        timeout
    )?;
    Ok(())
}
