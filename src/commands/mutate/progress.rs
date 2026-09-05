use crate::engines::MutantOutcome;
use anyhow::Result;
use colored::*;
use std::io::Write;
use std::path::PathBuf;

pub(crate) fn print_generation_notice(files: &[PathBuf], diff: bool) -> Result<()> {
    writeln!(
        std::io::stdout().lock(),
        "{} generating AST mutations across {} source files (diff: {})...",
        "note:".bold(),
        files.len().to_string().cyan(),
        diff
    )?;
    Ok(())
}

pub(crate) fn print_mutant_notice(count: usize, timeout: u64) -> Result<()> {
    writeln!(
        std::io::stdout().lock(),
        "{} running {} mutants (timeout: {}s per mutant)...",
        "note:".bold(),
        count.to_string().cyan(),
        timeout
    )?;
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum OutcomeStyle {
    Green,
    Red,
    Yellow,
}

pub(crate) fn outcome_label(outcome: MutantOutcome) -> (&'static str, OutcomeStyle) {
    match outcome {
        MutantOutcome::Killed => ("killed", OutcomeStyle::Green),
        MutantOutcome::Survived => ("survived", OutcomeStyle::Red),
        MutantOutcome::Timeout => ("timeout", OutcomeStyle::Yellow),
        MutantOutcome::CompileError => ("compile error", OutcomeStyle::Red),
        MutantOutcome::RunnerError => ("runner error", OutcomeStyle::Red),
        MutantOutcome::Equivalent => ("equivalent", OutcomeStyle::Yellow),
        MutantOutcome::Unviable => ("unviable", OutcomeStyle::Red),
    }
}

pub(crate) fn increment_stats(stats: &mut crate::engines::MutationStats, outcome: MutantOutcome) {
    match outcome {
        MutantOutcome::Killed => stats.killed += 1,
        MutantOutcome::Survived => stats.survived += 1,
        MutantOutcome::Timeout => stats.timeout += 1,
        MutantOutcome::CompileError => stats.compile_error += 1,
        MutantOutcome::RunnerError => stats.runner_error += 1,
        MutantOutcome::Equivalent => stats.equivalent += 1,
        MutantOutcome::Unviable => stats.unviable += 1,
    }
}

pub(crate) fn print_outcome(
    stats: &mut crate::engines::MutationStats,
    outcome: MutantOutcome,
) -> Result<()> {
    increment_stats(stats, outcome);
    let (label, style) = outcome_label(outcome);
    let label = match style {
        OutcomeStyle::Green => label.green().bold(),
        OutcomeStyle::Red => label.red().bold(),
        OutcomeStyle::Yellow => label.yellow().bold(),
    };
    writeln!(std::io::stdout().lock(), "{label}")?;
    Ok(())
}
