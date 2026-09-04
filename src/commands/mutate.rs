use super::mutation_output::{MutationSummaryContext, render_mutation_output};
use crate::config::HardgateConfig;
use crate::discovery::{ClassifiedFile, DiscoverOptions, discover_files};
use crate::engines::{
    AstMutant, AstMutationGenerator, BaselineOutcome, MutantExecutionResult, MutantOutcome,
    MutationStats, NativeMutationRunner,
};
use anyhow::{Context, Result, bail};
use colored::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// CLI options for `hardgate mutate`.
#[derive(Debug, Clone, Default)]
pub struct MutateOptions {
    pub diff: bool,
    pub scoped: Option<PathBuf>,
    pub test_cmd: Option<String>,
    pub timeout_secs: Option<u64>,
    pub max_mutants: Option<usize>,
    pub format: Option<String>,
}

/// Run native Tree-sitter AST mutation testing: generate mutants, execute the
/// test suite per mutant with timeouts and RAII rollbacks, then report the
/// kill score. Exits non-zero below the configured floor.
pub fn cmd_mutate(opts: MutateOptions) -> Result<()> {
    let start_time = Instant::now();
    let config = HardgateConfig::load_or_default(None)?;
    let root = Path::new(".");

    if !config.mutation.enabled {
        println!(
            "{} mutation testing is disabled by `[mutation].enabled = false`.",
            "note:".green().bold()
        );
        return Ok(());
    }

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
    if max_count == 0 {
        bail!("mutation max_mutants must be greater than zero");
    }

    let timeout = opts
        .timeout_secs
        .or(config.mutation.timeout_secs)
        .unwrap_or(10);
    if timeout == 0 {
        bail!("mutation timeout_secs must be greater than zero");
    }
    let test_cmd = opts
        .test_cmd
        .clone()
        .or_else(|| config.mutation.test_cmd.clone());
    let runner = NativeMutationRunner::new(timeout, test_cmd);

    run_unmutated_baselines(&runner, &target_files, root)?;
    let mutants = generate_target_mutants(&target_files, max_count)?;
    if mutants.is_empty() {
        bail!("no viable AST mutation points were found in the selected production sources");
    }

    println!(
        "{} running {} mutants (timeout: {}s per mutant)...",
        "note:".bold(),
        mutants.len().to_string().cyan(),
        timeout
    );

    let (results, stats) = run_mutant_batch(&mutants, &runner, root)?;
    let score = stats.score_percent();
    let min_score = config.mutation.min_score.unwrap_or(85.0);
    let passed = mutation_run_passed(&stats, score, min_score);
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

fn discover_targets(
    opts: &MutateOptions,
    config: &HardgateConfig,
    root: &Path,
) -> Result<Vec<PathBuf>> {
    if let Some(ref path) = opts.scoped {
        if path.is_file() {
            let classified = ClassifiedFile::new(path);
            if !classified.role.is_mutation_target() {
                bail!(
                    "refusing to mutate `{}` because it is classified as {:?}, not production source",
                    path.display(),
                    classified.role
                );
            }
            if !classified.ast_supported {
                bail!(
                    "refusing to mutate `{}` because Hardgate has no AST mutator for its file type",
                    path.display()
                );
            }
            return Ok(vec![path.clone()]);
        }
        if path.is_dir() {
            let files = discover_files(DiscoverOptions {
                root: path,
                diff_only: false,
                exclusions: &config.budgets.files.exclusions.paths,
            })?;
            return Ok(filter_production_sources(files));
        }
        anyhow::bail!("Path not found: {:?}", path);
    }
    let files = discover_files(DiscoverOptions {
        root,
        diff_only: opts.diff,
        exclusions: &config.budgets.files.exclusions.paths,
    })?;
    Ok(filter_production_sources(files))
}

fn filter_production_sources(files: Vec<PathBuf>) -> Vec<PathBuf> {
    files
        .into_iter()
        .filter(|path| {
            let classified = ClassifiedFile::new(path);
            classified.role.is_mutation_target() && classified.ast_supported
        })
        .collect()
}

fn run_unmutated_baselines(
    runner: &NativeMutationRunner,
    files: &[PathBuf],
    root: &Path,
) -> Result<()> {
    let mut commands = BTreeMap::new();
    for file in files {
        commands
            .entry(runner.resolve_test_command(file, root))
            .or_insert_with(|| file.clone());
    }

    println!(
        "{} running {} unmutated baseline command(s)...",
        "note:".bold(),
        commands.len().to_string().cyan()
    );
    for (command, file) in commands {
        let result = runner.run_baseline(&file, root);
        if result.outcome == BaselineOutcome::Passed {
            println!("   {} ... {}", command.dimmed(), "passed".green().bold());
            continue;
        }
        let diagnostic = if result.diagnostic.trim().is_empty() {
            "no diagnostic output".to_string()
        } else {
            result.diagnostic
        };
        bail!(
            "unmutated baseline {:?} for `{}` using `{}`:\n{}",
            result.outcome,
            file.display(),
            result.command,
            diagnostic
        );
    }
    Ok(())
}

fn generate_target_mutants(files: &[PathBuf], max_count: usize) -> Result<Vec<AstMutant>> {
    let mut mutator = AstMutationGenerator::new();
    let mut all = Vec::new();
    for file in files {
        let content = fs::read_to_string(file)
            .with_context(|| format!("Failed to read mutation target `{}`", file.display()))?;
        all.extend(mutator.generate_mutants(file, &content));
    }
    Ok(select_representative_mutants(all, max_count))
}

fn select_representative_mutants(
    mut candidates: Vec<AstMutant>,
    max_count: usize,
) -> Vec<AstMutant> {
    candidates.sort_by(|left, right| {
        (
            &left.file,
            left.line,
            left.column,
            &left.original,
            &left.replacement,
        )
            .cmp(&(
                &right.file,
                right.line,
                right.column,
                &right.original,
                &right.replacement,
            ))
    });

    let mut per_family_rank = BTreeMap::new();
    let mut ranked = Vec::with_capacity(candidates.len());
    for mutant in candidates {
        let family = (
            mutant.file.clone(),
            mutant.original.clone(),
            mutant.replacement.clone(),
        );
        let rank = per_family_rank.entry(family).or_insert(0_usize);
        ranked.push((*rank, mutant));
        *rank += 1;
    }
    ranked.sort_by(|(left_rank, left), (right_rank, right)| {
        (
            left_rank,
            &left.file,
            &left.original,
            &left.replacement,
            left.line,
            left.column,
        )
            .cmp(&(
                right_rank,
                &right.file,
                &right.original,
                &right.replacement,
                right.line,
                right.column,
            ))
    });
    ranked
        .into_iter()
        .take(max_count)
        .enumerate()
        .map(|(index, (_, mut mutant))| {
            mutant.id = index + 1;
            mutant
        })
        .collect()
}

fn run_mutant_batch(
    mutants: &[AstMutant],
    runner: &NativeMutationRunner,
    root: &Path,
) -> Result<(Vec<MutantExecutionResult>, MutationStats)> {
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
            MutantOutcome::CompileError => {
                stats.compile_error += 1;
                println!("{}", "compile error".red().bold());
            }
            MutantOutcome::RunnerError => {
                stats.runner_error += 1;
                println!("{}", "runner error".red().bold());
            }
            MutantOutcome::Equivalent => {
                stats.equivalent += 1;
                println!("{}", "equivalent".yellow());
            }
            MutantOutcome::Unviable => {
                stats.unviable += 1;
                println!("{}", "unviable".red().bold());
            }
        }
        results.push(res);
    }

    Ok((results, stats))
}

fn mutation_run_passed(stats: &MutationStats, score: f64, min_score: f64) -> bool {
    let viable = stats.killed + stats.survived;
    viable > 0
        && score >= min_score
        && stats.timeout == 0
        && stats.compile_error == 0
        && stats.runner_error == 0
        && stats.unviable == 0
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
