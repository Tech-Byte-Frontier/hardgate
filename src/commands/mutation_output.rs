use crate::engines::{MutantExecutionResult, MutantOutcome, MutationStats};
use colored::*;

/// Borrowed inputs for rendering one mutation run in any output mode.
pub struct MutationSummaryContext<'a> {
    pub stats: &'a MutationStats,
    pub results: &'a [MutantExecutionResult],
    pub score: f64,
    pub min_score: f64,
    pub passed: bool,
    pub elapsed: u128,
}

pub(crate) fn render_mutation_output(ctx: &MutationSummaryContext, format: Option<&str>) {
    match format {
        Some("agent") => render_agent_output(ctx),
        Some("json") => render_json_output(ctx),
        _ => print!("{}", format_mutation_terminal(ctx)),
    }
}

fn render_json_output(ctx: &MutationSummaryContext) {
    let value = serde_json::json!({
        "stats": ctx.stats,
        "score": ctx.score,
        "min_score": ctx.min_score,
        "passed": ctx.passed,
        "duration_ms": ctx.elapsed,
        "results": ctx.results,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_default()
    );
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
