use crate::engines::mutation::runner::MutationRunnerError;
use crate::engines::mutation::{BaselineExecutionResult, BaselineOutcome};
use crate::engines::{MutantExecutionResult, MutantOutcome, MutationStats};
use colored::*;
use serde::Serialize;
use std::fmt;
use std::path::Path;

/// Borrowed inputs for rendering one mutation run in any output mode.
pub struct MutationSummaryContext<'a> {
    pub stats: &'a MutationStats,
    pub results: &'a [MutantExecutionResult],
    pub score: f64,
    pub min_score: f64,
    pub passed: bool,
    pub elapsed: u128,
}

#[derive(Debug)]
pub struct MutationFailure {
    pub stage: &'static str,
    pub kind: &'static str,
    pub message: String,
}

impl MutationFailure {
    pub(crate) fn new(stage: &'static str, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            stage,
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn from_runner_error(error: MutationRunnerError) -> Self {
        match error {
            MutationRunnerError::Resolution(message) => {
                Self::new("resolution", "resolution-error", message)
            }
            MutationRunnerError::Integrity(message) => {
                Self::new("execution", "execution-error", message)
            }
        }
    }
}

impl fmt::Display for MutationFailure {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(&self.message)
    }
}

impl std::error::Error for MutationFailure {}

pub(crate) fn render_mutation_output(
    ctx: &MutationSummaryContext,
    format: Option<&str>,
) -> Result<(), serde_json::Error> {
    match format {
        Some("agent") => {
            render_agent_output(ctx);
            Ok(())
        }
        Some("json") => render_json_output(ctx),
        _ => {
            print!("{}", format_mutation_terminal(ctx));
            Ok(())
        }
    }
}

fn render_json_output(ctx: &MutationSummaryContext) -> Result<(), serde_json::Error> {
    println!(
        "{}",
        serde_json::to_string_pretty(&MutationJson {
            stats: ctx.stats,
            score: ctx.score,
            min_score: ctx.min_score,
            passed: ctx.passed,
            duration_ms: ctx.elapsed,
            results: ctx.results,
        })?
    );
    Ok(())
}

fn render_agent_output(ctx: &MutationSummaryContext) {
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
    print!("{out}");
}

#[derive(Serialize)]
struct MutationJson<'a> {
    stats: &'a MutationStats,
    score: f64,
    min_score: f64,
    passed: bool,
    duration_ms: u128,
    results: &'a [MutantExecutionResult],
}

#[derive(Serialize)]
pub(crate) struct MutationNoop<'a> {
    pub passed: bool,
    pub status: &'static str,
    pub stage: &'static str,
    pub kind: &'static str,
    pub message: &'a str,
}

const DISABLED_MUTATION_MESSAGE: &str =
    "mutation testing is disabled by \u{60}[mutation].enabled = false\u{60}.";
const NO_CHANGED_TARGETS_MESSAGE: &str =
    "no git-modified files found for mutation testing; no changed production source targets.";
const DISABLED_MUTATION_NOTICE: MutationNoopNotice = MutationNoopNotice {
    stage: "policy",
    kind: "disabled",
    message: DISABLED_MUTATION_MESSAGE,
    note: DISABLED_MUTATION_MESSAGE,
};
const NO_CHANGED_TARGETS_NOTICE: MutationNoopNotice = MutationNoopNotice {
    stage: "selection",
    kind: "no-changed-targets",
    message: NO_CHANGED_TARGETS_MESSAGE,
    note: "no git-modified files found for mutation testing; no changed production source targets (no-op).",
};

struct MutationNoopNotice {
    stage: &'static str,
    kind: &'static str,
    message: &'static str,
    note: &'static str,
}

pub(crate) fn render_mutation_noop(
    noop: MutationNoop<'_>,
    format: Option<&str>,
) -> Result<(), serde_json::Error> {
    if format == Some("json") {
        println!("{}", serde_json::to_string_pretty(&noop)?);
    }
    Ok(())
}

pub(crate) fn finish_disabled_mutation(format: Option<&str>) -> anyhow::Result<()> {
    render_noop_or_note(format, DISABLED_MUTATION_NOTICE)
}

pub(crate) fn handle_no_targets(diff: bool, format: Option<&str>) -> anyhow::Result<()> {
    if !diff {
        return Err(MutationFailure::new(
            "setup",
            "setup-error",
            "no source files found for mutation testing: no production source files are eligible; full/native runs require at least one production target",
        )
        .into());
    }
    render_noop_or_note(format, NO_CHANGED_TARGETS_NOTICE)
}

fn render_noop_or_note(format: Option<&str>, notice: MutationNoopNotice) -> anyhow::Result<()> {
    if format == Some("json") {
        render_mutation_noop(
            MutationNoop {
                passed: true,
                status: "noop",
                stage: notice.stage,
                kind: notice.kind,
                message: notice.message,
            },
            format,
        )
        .map_err(|error| MutationFailure::new("execution", "execution-error", error.to_string()))?;
    } else {
        println!("{} {}", "note:".green().bold(), notice.note);
    }
    Ok(())
}

pub(crate) fn baseline_failure(result: &BaselineExecutionResult, file: &Path) -> anyhow::Error {
    let diagnostic = if result.diagnostic.trim().is_empty() {
        "no diagnostic output".to_string()
    } else {
        result.diagnostic.clone()
    };
    let kind = match result.outcome {
        BaselineOutcome::Failed => "test-failure",
        BaselineOutcome::Timeout => "timeout",
        BaselineOutcome::RunnerError => "runner-error",
        BaselineOutcome::Passed => "test-failure",
    };
    MutationFailure::new(
        "baseline",
        kind,
        format!(
            "unmutated baseline {:?} for \u{60}{}\u{60} using \u{60}{}\u{60}:\n{}",
            result.outcome,
            file.display(),
            result.command,
            diagnostic
        ),
    )
    .into()
}

pub(crate) fn runtime_failure(result: &MutantExecutionResult) -> Option<anyhow::Error> {
    let kind = match result.outcome {
        MutantOutcome::RunnerError => "execution-error",
        MutantOutcome::Timeout => "timeout",
        MutantOutcome::Killed
        | MutantOutcome::Survived
        | MutantOutcome::CompileError
        | MutantOutcome::Equivalent
        | MutantOutcome::Unviable => return None,
    };
    Some(
        MutationFailure::new(
            "execution",
            kind,
            format!(
                "mutant {} {:?} for \u{60}{}\u{60}: {}",
                result.mutant.id,
                result.outcome,
                result.mutant.file.display(),
                if result.diagnostic.trim().is_empty() {
                    "no diagnostic output"
                } else {
                    result.diagnostic.as_str()
                }
            ),
        )
        .into(),
    )
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
