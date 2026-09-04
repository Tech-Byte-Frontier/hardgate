use super::super::mutation_output::{MutationFailure, baseline_failure};
use crate::engines::mutation::runner::BaselineRunContext;
use crate::engines::{BaselineOutcome, NativeMutationRunner};
use anyhow::Result;
use colored::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(super) struct BaselineRun<'a> {
    pub(super) runner: &'a NativeMutationRunner,
    pub(super) command_files: &'a [PathBuf],
    pub(super) protected_files: &'a [PathBuf],
    pub(super) root: &'a Path,
    pub(super) json: bool,
}

pub(super) fn run_unmutated_baselines(run: BaselineRun<'_>) -> Result<()> {
    let protected = NativeMutationRunner::snapshot_baseline_sources(run.protected_files, run.root)
        .map_err(|error| {
            MutationFailure::new(
                "baseline",
                "source-integrity-error",
                format!("failed to snapshot protected production sources before baseline: {error}"),
            )
        })?;
    let mut commands = BTreeMap::new();
    for file in run.command_files {
        let plan = run
            .runner
            .resolve_baseline_plan(file, run.root, &protected)
            .map_err(MutationFailure::from_runner_error)?;
        commands
            .entry((plan.working_dir.clone(), plan.command.clone()))
            .or_insert_with(|| (file.clone(), plan));
    }

    if !run.json {
        println!(
            "{} running {} unmutated baseline command(s)...",
            "note:".bold(),
            commands.len().to_string().cyan()
        );
    }
    for ((working_dir, command), (file, plan)) in commands {
        if !run.json {
            println!(
                "   {} ({}) in {}",
                command.dimmed(),
                plan.selection.description().dimmed(),
                working_dir.display()
            );
        }
        let result = run
            .runner
            .run_resolved_baseline_with_sources(BaselineRunContext::new(
                &file, run.root, &protected, plan,
            ));
        if result.outcome == BaselineOutcome::Passed {
            if !run.json {
                println!("      ... {}", "passed".green().bold());
            }
            continue;
        }
        return Err(baseline_failure(&result, &file));
    }
    Ok(())
}
