use crate::config::HardgateConfig;
use crate::discovery::{DiscoverOptions, discover_files};
use crate::engines::{
    AstMutant, AstMutationGenerator, MutantExecutionResult, MutantOutcome, MutationStats,
    NativeMutationRunner,
};
use anyhow::Result;
use colored::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct MutateOptions {
    pub diff: bool,
    pub scoped: Option<PathBuf>,
    pub test_cmd: Option<String>,
    pub timeout_secs: Option<u64>,
    pub max_mutants: Option<usize>,
    pub format: Option<String>,
}

pub fn cmd_mutate(opts: MutateOptions) -> Result<()> {
    let start_time = Instant::now();
    let config = HardgateConfig::load_or_default(None)?;
    let root = Path::new(".");

    let target_files = discover_targets(&opts, &config, root)?;
    if target_files.is_empty() {
        print_no_targets(opts.diff);
        return Ok(());
    }

    println!(
        "{} generating AST mutations across {} source files (diff: {})...",
        "note:".bold(),
        target_files.len().to_string().cyan(),
        opts.diff
    );

    let max_count = opts
        .max_mutants
        .or(config.mutation.max_mutants)
        .unwrap_or(30);
    let mutants = generate_target_mutants(&target_files, max_count);
    if mutants.is_empty() {
        println!(
            "{} no candidate AST mutation points found in selected files.",
            "warning:".yellow().bold()
        );
        return Ok(());
    }

    let timeout = opts
        .timeout_secs
        .or(config.mutation.timeout_secs)
        .unwrap_or(10);
    let test_cmd = opts.test_cmd.or_else(|| config.mutation.test_cmd.clone());

    println!(
        "{} running {} mutants (timeout: {}s per mutant)...",
        "note:".bold(),
        mutants.len().to_string().cyan(),
        timeout
    );

    let (results, stats) = run_mutant_batch(&mutants, timeout, test_cmd, root)?;
    let score = stats.score_percent();
    let min_score = config.mutation.min_score.unwrap_or(85.0);
    let passed = score >= min_score && (!config.mutation.reject_timeouts || stats.timeout == 0);
    let elapsed = start_time.elapsed().as_millis();

    let summary_ctx = MutationSummaryContext {
        stats: &stats,
        results: &results,
        score,
        min_score,
        passed,
        elapsed,
    };

    render_mutation_output(&summary_ctx, opts.format.as_deref());

    if !passed {
        std::process::exit(1);
    }
    Ok(())
}

struct MutationSummaryContext<'a> {
    stats: &'a MutationStats,
    results: &'a [MutantExecutionResult],
    score: f64,
    min_score: f64,
    passed: bool,
    elapsed: u128,
}

fn discover_targets(
    opts: &MutateOptions,
    config: &HardgateConfig,
    root: &Path,
) -> Result<Vec<PathBuf>> {
    if let Some(ref path) = opts.scoped {
        if path.is_file() {
            return Ok(vec![path.clone()]);
        }
        if path.is_dir() {
            return discover_files(DiscoverOptions {
                root: path,
                diff_only: false,
                exclusions: &config.budgets.files.exclusions.paths,
            });
        }
        anyhow::bail!("Path not found: {:?}", path);
    }
    discover_files(DiscoverOptions {
        root,
        diff_only: opts.diff,
        exclusions: &config.budgets.files.exclusions.paths,
    })
}

fn generate_target_mutants(files: &[PathBuf], max_count: usize) -> Vec<AstMutant> {
    let mut mutator = AstMutationGenerator::new();
    let mut all = Vec::new();
    for file in files {
        if let Ok(content) = fs::read_to_string(file) {
            all.extend(mutator.generate_mutants(file, &content));
        }
    }
    all.into_iter().take(max_count).collect()
}

fn run_mutant_batch(
    mutants: &[AstMutant],
    timeout: u64,
    test_cmd: Option<String>,
    root: &Path,
) -> Result<(Vec<MutantExecutionResult>, MutationStats)> {
    let runner = NativeMutationRunner::new(timeout, test_cmd);
    let mut results = Vec::new();
    let mut stats = MutationStats {
        total: mutants.len(),
        ..Default::default()
    };

    for (idx, mutant) in mutants.iter().enumerate() {
        print!(
            "   [{}/{}] {}:{} {} ... ",
            idx + 1,
            mutants.len(),
            mutant.file.display().to_string().bold(),
            mutant.line.to_string().yellow(),
            mutant.description.dimmed()
        );
        std::io::Write::flush(&mut std::io::stdout())?;

        let res = runner.run_mutant(mutant, root);
        match res.outcome {
            MutantOutcome::Killed => {
                stats.killed += 1;
                println!("{}", "killed".green().bold());
            }
            MutantOutcome::Survived => {
                stats.survived += 1;
                println!("{}", "survived".red().bold());
            }
            MutantOutcome::Timeout => {
                stats.timeout += 1;
                println!("{}", "timeout".yellow().bold());
            }
            MutantOutcome::Error => {
                println!("{}", "error".red());
            }
        }
        results.push(res);
    }

    Ok((results, stats))
}

fn print_no_targets(diff: bool) {
    if diff {
        println!(
            "{} no git-modified files found for mutation testing.",
            "note:".green().bold()
        );
    } else {
        println!(
            "{} no source files found for mutation testing.",
            "warning:".yellow().bold()
        );
    }
}

fn render_mutation_output(ctx: &MutationSummaryContext, format: Option<&str>) {
    match format {
        Some("agent") => render_agent_output(ctx),
        Some("json") => {
            let json_obj = serde_json::json!({
                "stats": ctx.stats,
                "score": ctx.score,
                "min_score": ctx.min_score,
                "passed": ctx.passed,
                "duration_ms": ctx.elapsed,
                "results": ctx.results,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json_obj).unwrap_or_default()
            );
        }
        _ => render_terminal_output(ctx),
    }
}

fn render_agent_output(ctx: &MutationSummaryContext) {
    let mut out = format!(
        "### 🧬 Native AST Mutation Results ({}ms)\n- Evaluated: {}\n- Killed: {}\n- Survived: {}\n- Timed Out: {}\n- Mutation Score: {:.1}% (Floor: {:.1}%)\n- Verdict: {}\n\n",
        ctx.elapsed,
        ctx.stats.total,
        ctx.stats.killed,
        ctx.stats.survived,
        ctx.stats.timeout,
        ctx.score,
        ctx.min_score,
        if ctx.passed { "PASSED" } else { "FAILED" }
    );
    for res in ctx
        .results
        .iter()
        .filter(|r| r.outcome == MutantOutcome::Survived)
    {
        out.push_str(&format!(
            "- ⚠️ Survived Mutant in `{}:{}`: {}\n  Original: `{}` -> Mutant: `{}`\n  Directive: Add a test asserting behavior for this case.\n",
            res.mutant.file.display(), res.mutant.line, res.mutant.description, res.mutant.original, res.mutant.replacement
        ));
    }
    print!("{}", out);
}

fn render_terminal_output(ctx: &MutationSummaryContext) {
    println!("\n{}", "-".repeat(70).dimmed());
    println!("{}", "mutation summary:".bold());
    println!("  mutants tested:  {}", ctx.stats.total.to_string().cyan());
    println!(
        "  killed:          {}",
        ctx.stats.killed.to_string().green()
    );
    println!(
        "  survived:        {}",
        ctx.stats.survived.to_string().red()
    );
    println!(
        "  timed out:       {}",
        ctx.stats.timeout.to_string().yellow()
    );
    println!(
        "  score:           {:.1}% (threshold: {:.1}%)",
        ctx.score, ctx.min_score
    );
    println!(
        "  result:          {}",
        if ctx.passed {
            "pass".bold().green()
        } else {
            "fail".bold().red()
        }
    );

    let survivors: Vec<_> = ctx
        .results
        .iter()
        .filter(|r| r.outcome == MutantOutcome::Survived)
        .collect();
    if !survivors.is_empty() {
        println!(
            "\n{} {}",
            "warning:".yellow().bold(),
            format!("survived mutants ({})", survivors.len()).bold()
        );
        for res in survivors {
            println!(
                "  --> {}:{}: {}\n       original: `{}` mutated: `{}`\n       {} add a test asserting behavior for this code branch.\n",
                res.mutant.file.display().to_string().bold(),
                res.mutant.line.to_string().yellow(),
                res.mutant.description,
                res.mutant.original.red(),
                res.mutant.replacement.green(),
                "help:".dimmed(),
            );
        }
    }
}
