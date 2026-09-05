use super::outcome::CommandOutcome;
use crate::diagnostics::execution::{EngineId, EngineState, ExecutionPlan};
use crate::engines::{MutantExecutionResult, MutantOutcome, MutationStats};
use colored::*;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[path = "mutate/failure.rs"]
mod failure;
pub use failure::*;

/// Borrowed inputs for rendering one mutation run in any output mode.
pub struct MutationSummaryContext<'a> {
    pub stats: &'a MutationStats,
    pub results: &'a [MutantExecutionResult],
    pub score: f64,
    pub min_score: f64,
    pub passed: bool,
    pub elapsed: u128,
}

#[derive(Debug, Clone, Default)]
pub struct MutationRenderOptions<'a> {
    pub format: Option<&'a str>,
    pub summary: bool,
    pub output_file: Option<&'a Path>,
}

#[cfg(test)]
pub(crate) fn render_mutation_output(
    ctx: &MutationSummaryContext,
    format: Option<&str>,
    execution: Option<&ExecutionPlan>,
) -> anyhow::Result<()> {
    render_mutation_output_with_options(
        ctx,
        MutationRenderOptions {
            format,
            summary: false,
            output_file: None,
        },
        execution,
    )
}

pub(crate) fn render_mutation_output_with_options(
    ctx: &MutationSummaryContext,
    opts: MutationRenderOptions<'_>,
    execution: Option<&ExecutionPlan>,
) -> anyhow::Result<()> {
    let output = format_mutation_content(ctx, opts.format, opts.summary, execution)?;
    if let Some(path) = opts.output_file {
        super::outcome::write_atomic_file(path, &output)?;
    }
    write!(std::io::stdout().lock(), "{output}")?;
    Ok(())
}

fn format_mutation_content(
    ctx: &MutationSummaryContext,
    format: Option<&str>,
    summary: bool,
    execution: Option<&ExecutionPlan>,
) -> anyhow::Result<String> {
    match format {
        Some("agent") => Ok(format_agent_output(ctx)),
        Some("json") if summary => render_json_summary(ctx, execution),
        Some("json") => render_json_full(ctx, execution),
        _ if summary => Ok(format_mutation_summary_terminal(ctx)),
        _ => Ok(format_mutation_terminal(ctx)),
    }
}

#[derive(Debug, Serialize)]
struct MutationSummaryJson<'a> {
    schema_version: u32,
    command: &'static str,
    status: &'static str,
    exit_code: u8,
    passed: bool,
    score: f64,
    min_score: f64,
    duration_ms: u128,
    stats: &'a MutationStats,
    all_sources_restored: bool,
    survivors: Vec<SurvivorSummary>,
}

#[derive(Debug, Serialize)]
struct SurvivorSummary {
    id: usize,
    file: String,
    line: usize,
    description: String,
    original: String,
    replacement: String,
}

fn render_json_summary(
    ctx: &MutationSummaryContext,
    _execution: Option<&ExecutionPlan>,
) -> anyhow::Result<String> {
    let outcome = ctx.outcome();
    let all_sources_restored = ctx.results.iter().all(|r| r.source_restored);
    let survivors: Vec<_> = ctx
        .results
        .iter()
        .filter(|r| r.outcome == MutantOutcome::Survived)
        .map(|r| SurvivorSummary {
            id: r.mutant.id,
            file: r.mutant.file.display().to_string(),
            line: r.mutant.line,
            description: r.mutant.description.clone(),
            original: r.mutant.original.clone(),
            replacement: r.mutant.replacement.clone(),
        })
        .collect();

    let json = serde_json::to_string_pretty(&MutationSummaryJson {
        schema_version: 1,
        command: "mutate",
        status: outcome.status(),
        exit_code: outcome.exit_code(),
        passed: ctx.passed,
        score: ctx.score,
        min_score: ctx.min_score,
        duration_ms: ctx.elapsed,
        stats: ctx.stats,
        all_sources_restored,
        survivors,
    })?;
    Ok(format!("{json}\n"))
}

fn render_json_full(
    ctx: &MutationSummaryContext,
    execution: Option<&ExecutionPlan>,
) -> anyhow::Result<String> {
    let outcome = ctx.outcome();
    let execution = mutation_execution(execution, outcome);
    let json = serde_json::to_string_pretty(&MutationJson {
        schema_version: 1,
        command: "mutate",
        status: outcome.status(),
        exit_code: outcome.exit_code(),
        execution: execution.as_ref(),
        stats: ctx.stats,
        score: ctx.score,
        min_score: ctx.min_score,
        passed: ctx.passed,
        duration_ms: ctx.elapsed,
        results: ctx.results,
    })?;
    Ok(format!("{json}\n"))
}

pub fn format_mutation_summary_terminal(ctx: &MutationSummaryContext) -> String {
    let mut out = format_mutation_terminal(ctx);
    let all_restored = ctx.results.iter().all(|r| r.source_restored);
    if all_restored {
        out.push_str(&format!(
            "\nrestoration: all {} mutated sources successfully restored to baseline\n",
            ctx.results.len()
        ));
    } else {
        out.push_str(&format!(
            "\n{}\n",
            "warning: some source files failed restoration verification!".yellow()
        ));
    }
    out
}

fn format_agent_output(ctx: &MutationSummaryContext) -> String {
    let mut out = format!(
        "### 🧬 Native AST Mutation Results ({}ms)\n- Evaluated: {}\n- Killed: {}\n- Survived: {}\n- Timed Out: {}\n- Compile Errors: {}\n- Runner Errors: {}\n- Equivalent: {}\n- Unviable: {}\n- Mutation Score: {:.1}% (Floor: {:.1}%)\n- Verdict: {}\n\n",
        ctx.elapsed,
        ctx.stats.total,
        ctx.stats.killed,
        ctx.stats.survived,
        ctx.stats.timeout,
        ctx.stats.compile_error,
        ctx.stats.runner_error,
        ctx.stats.equivalent,
        ctx.stats.unviable,
        ctx.score,
        ctx.min_score,
        if ctx.passed { "PASSED" } else { "FAILED" }
    );
    for result in ctx
        .results
        .iter()
        .filter(|result| result.outcome == MutantOutcome::Survived)
    {
        out.push_str(&format!(
            "- ⚠️ Survived Mutant in `{}:{}`: {}\n  Original: `{}` -> Mutant: `{}`\n  Directive: Add a test asserting behavior for this case.\n",
            result.mutant.file.display(),
            result.mutant.line,
            result.mutant.description,
            result.mutant.original,
            result.mutant.replacement
        ));
    }
    out
}

#[derive(Serialize)]
struct MutationJson<'a> {
    schema_version: u32,
    command: &'static str,
    status: &'static str,
    exit_code: u8,
    execution: Option<&'a ExecutionPlan>,
    stats: &'a MutationStats,
    score: f64,
    min_score: f64,
    passed: bool,
    duration_ms: u128,
    results: &'a [MutantExecutionResult],
}

/// Terminal rendering of a mutation run as a plain string (testable).
/// The verdict repeats at the end so tail-only readers see the outcome.
pub fn format_mutation_terminal(ctx: &MutationSummaryContext) -> String {
    let mut out = format!(
        "\n{}\n{}\n  mutants tested:  {}\n  killed:          {}\n  survived:        {}\n  timed out:       {}\n  compile errors:  {}\n  runner errors:   {}\n  equivalent:      {}\n  unviable:        {}\n  score:           {:.1}% (threshold: {:.1}%)\n  result:          {}\n",
        "-".repeat(70).dimmed(),
        "mutation summary:".bold(),
        ctx.stats.total.to_string().cyan(),
        ctx.stats.killed.to_string().green(),
        ctx.stats.survived.to_string().red(),
        ctx.stats.timeout.to_string().yellow(),
        ctx.stats.compile_error.to_string().red(),
        ctx.stats.runner_error.to_string().red(),
        ctx.stats.equivalent.to_string().yellow(),
        ctx.stats.unviable.to_string().red(),
        ctx.score,
        ctx.min_score,
        if ctx.passed {
            "pass".bold().green()
        } else {
            "fail".bold().red()
        }
    );
    append_survivors(&mut out, ctx.results);
    append_closing_verdict(&mut out, ctx);
    out
}

fn append_survivors(out: &mut String, results: &[MutantExecutionResult]) {
    let survivors: Vec<_> = results
        .iter()
        .filter(|result| result.outcome == MutantOutcome::Survived)
        .collect();
    if survivors.is_empty() {
        return;
    }
    out.push_str(&format!(
        "\n{} {}\n",
        "warning:".yellow().bold(),
        format!("survived mutants ({})", survivors.len()).bold()
    ));
    for result in survivors {
        out.push_str(&format!(
            "  --> {}:{}: {}\n       original: `{}` mutated: `{}`\n       {} add a test asserting behavior for this code branch.\n",
            result.mutant.file.display().to_string().bold(),
            result.mutant.line.to_string().yellow(),
            result.mutant.description,
            result.mutant.original.red(),
            result.mutant.replacement.green(),
            "help:".dimmed(),
        ));
    }
}

fn append_closing_verdict(out: &mut String, ctx: &MutationSummaryContext) {
    let verdict = if ctx.passed {
        "pass".bold().green()
    } else {
        "fail".bold().red()
    };
    out.push_str(&format!(
        "{}\nresult: {} · score {:.1}% (threshold: {:.1}%) · {} killed, {} survived, {} timed out · {} compile errors, {} runner errors, {} equivalent, {} unviable\n",
        "-".repeat(70).dimmed(),
        verdict,
        ctx.score,
        ctx.min_score,
        ctx.stats.killed,
        ctx.stats.survived,
        ctx.stats.timeout,
        ctx.stats.compile_error,
        ctx.stats.runner_error,
        ctx.stats.equivalent,
        ctx.stats.unviable
    ));
}

impl MutationSummaryContext<'_> {
    pub(crate) fn outcome(&self) -> CommandOutcome {
        if self.passed {
            CommandOutcome::Passed
        } else if self.stats.runner_error > 0
            || self.stats.timeout > 0
            || self.stats.compile_error > 0
            || self.stats.unviable > 0
            || self.stats.killed + self.stats.survived == 0
        {
            CommandOutcome::Incomplete
        } else {
            CommandOutcome::Violations
        }
    }
}

fn mutation_execution(
    plan: Option<&ExecutionPlan>,
    outcome: CommandOutcome,
) -> Option<ExecutionPlan> {
    let mut plan = plan?.clone();
    let state = match outcome {
        CommandOutcome::Passed => EngineState::Completed,
        CommandOutcome::Violations => EngineState::Failed,
        CommandOutcome::Incomplete => EngineState::Incomplete,
    };
    for engine in &mut plan.engines {
        if engine.id == EngineId::MutationExecution {
            engine.state = state;
            engine.reason = None;
        }
    }
    Some(plan)
}
