use super::mutation_output::{MutationSummaryContext, render_mutation_output};
use crate::config::HardgateConfig;
use crate::discovery::{ClassifiedFile, DiscoverOptions, discover_files};
use crate::engines::mutation::FULL_SUITE_TIMEOUT_SECS;
use crate::engines::mutation::target::is_effective_mutation_target;
use crate::engines::{
    AstMutant, AstMutationGenerator, BaselineExecutionResult, BaselineOutcome,
    MutantExecutionResult, MutantOutcome, MutationStats, NativeMutationRunner,
};
use anyhow::{Context, Result, bail};
use colored::*;
use std::collections::{BTreeMap, BTreeSet};
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

struct MutationRun<'a> {
    config: &'a HardgateConfig,
    opts: &'a MutateOptions,
    results: &'a [MutantExecutionResult],
    stats: &'a MutationStats,
    start_time: Instant,
}

/// Run native Tree-sitter AST mutation testing: generate mutants, execute the
/// test suite per mutant with timeouts and RAII rollbacks, then report the
/// kill score. Exits non-zero below the configured floor.
pub fn cmd_mutate(opts: MutateOptions) -> Result<()> {
    let start_time = Instant::now();
    let config = HardgateConfig::load_or_default(None)?;
    let root = Path::new(".");
    if !config.mutation.enabled {
        print_disabled_mutation();
        return Ok(());
    }
    let target_files = discover_targets(&opts, &config, root)?;
    if target_files.is_empty() {
        print_no_targets(opts.diff);
        return Ok(());
    }
    print_generation_notice(&target_files, opts.diff);
    let test_cmd = opts
        .test_cmd
        .clone()
        .or_else(|| config.mutation.test_cmd.clone());
    let max_count = resolve_max_mutants(&opts, &config)?;
    let timeout = resolve_timeout(&opts, &config, &target_files, root)?;
    let runner = NativeMutationRunner::new(timeout, test_cmd);
    run_unmutated_baselines(&runner, &target_files, root)?;
    let mutants = generate_target_mutants(&target_files, max_count)?;
    if mutants.is_empty() {
        bail!("no viable AST mutation points were found in the selected production sources");
    }
    print_mutant_notice(mutants.len(), timeout);
    let (results, stats) = run_mutant_batch(&mutants, &runner, root)?;
    finish_mutation_run(MutationRun {
        config: &config,
        opts: &opts,
        results: &results,
        stats: &stats,
        start_time,
    })
}

fn print_disabled_mutation() {
    println!(
        "{} mutation testing is disabled by `[mutation].enabled = false`.",
        "note:".green().bold()
    );
}

fn print_generation_notice(files: &[PathBuf], diff: bool) {
    println!(
        "{} generating AST mutations across {} source files (diff: {})...",
        "note:".bold(),
        files.len().to_string().cyan(),
        diff
    );
}

fn resolve_max_mutants(opts: &MutateOptions, config: &HardgateConfig) -> Result<usize> {
    let max_count = opts
        .max_mutants
        .or(config.mutation.max_mutants)
        .unwrap_or(30);
    if max_count == 0 {
        bail!("mutation max_mutants must be greater than zero");
    }
    Ok(max_count)
}

fn resolve_timeout(
    opts: &MutateOptions,
    config: &HardgateConfig,
    files: &[PathBuf],
    root: &Path,
) -> Result<u64> {
    let test_cmd = opts
        .test_cmd
        .as_deref()
        .or(config.mutation.test_cmd.as_deref());
    let configured_timeout = opts.timeout_secs.or(config.mutation.timeout_secs);
    if let Some(timeout) = configured_timeout {
        if timeout == 0 {
            bail!("mutation timeout_secs must be greater than zero");
        }
        if let Some(recommended) = automatic_full_suite_timeout(files, root, test_cmd)
            && timeout < recommended
        {
            bail!(
                "automatic JavaScript full-suite selection requires timeout_secs >= {recommended}s (configured {timeout}s); pass --timeout {recommended} or set [mutation].timeout_secs = {recommended} before baseline"
            );
        }
        return Ok(timeout);
    }
    Ok(NativeMutationRunner::default_timeout_secs(
        files, root, test_cmd,
    ))
}

fn automatic_full_suite_timeout(
    files: &[PathBuf],
    root: &Path,
    test_cmd: Option<&str>,
) -> Option<u64> {
    if test_cmd.is_some() {
        return None;
    }
    let runner = NativeMutationRunner::new(FULL_SUITE_TIMEOUT_SECS, None);
    files
        .iter()
        .map(|file| runner.resolve_test_plan(file, root))
        .filter(|plan| plan.full_suite_timeout_required())
        .map(|plan| plan.recommended_timeout_secs)
        .max()
}

fn print_mutant_notice(count: usize, timeout: u64) {
    println!(
        "{} running {} mutants (timeout: {}s per mutant)...",
        "note:".bold(),
        count.to_string().cyan(),
        timeout
    );
}

fn finish_mutation_run(run: MutationRun<'_>) -> Result<()> {
    let score = run.stats.score_percent();
    let min_score = run.config.mutation.min_score.unwrap_or(85.0);
    let passed = mutation_run_passed(run.stats, score, min_score);
    render_mutation_output(
        &MutationSummaryContext {
            stats: run.stats,
            results: run.results,
            score,
            min_score,
            passed,
            elapsed: run.start_time.elapsed().as_millis(),
        },
        run.opts.format.as_deref(),
    );
    if passed {
        Ok(())
    } else {
        std::process::exit(1)
    }
}

fn discover_targets(
    opts: &MutateOptions,
    config: &HardgateConfig,
    root: &Path,
) -> Result<Vec<PathBuf>> {
    if let Some(ref path) = opts.scoped {
        if path.is_file() {
            let classified = ClassifiedFile::new_with_config(path, &config.classification)?;
            if !is_effective_mutation_target(&classified, config) {
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
            return filter_production_sources(files, config);
        }
        anyhow::bail!("Path not found: {:?}", path);
    }
    let files = discover_files(DiscoverOptions {
        root,
        diff_only: opts.diff,
        exclusions: &config.budgets.files.exclusions.paths,
    })?;
    filter_production_sources(files, config)
}

fn filter_production_sources(files: Vec<PathBuf>, config: &HardgateConfig) -> Result<Vec<PathBuf>> {
    let mut targets = Vec::new();
    for path in files {
        let classified = ClassifiedFile::new_with_config(&path, &config.classification)?;
        if is_effective_mutation_target(&classified, config) && classified.ast_supported {
            targets.push(path);
        }
    }
    Ok(targets)
}

/// Resolve whether a path is an effective native mutation target under the
/// built-in role default and any configured role policy override.
pub fn effective_mutation_target(path: &Path, config: &HardgateConfig) -> Result<bool> {
    let classified = ClassifiedFile::new_with_config(path, &config.classification)?;
    Ok(is_effective_mutation_target(&classified, config))
}

fn run_unmutated_baselines(
    runner: &NativeMutationRunner,
    files: &[PathBuf],
    root: &Path,
) -> Result<()> {
    let mut commands = BTreeMap::new();
    for file in files {
        let plan = runner.resolve_test_plan(file, root);
        commands
            .entry((plan.working_dir, plan.command))
            .or_insert_with(|| file.clone());
    }

    println!(
        "{} running {} unmutated baseline command(s)...",
        "note:".bold(),
        commands.len().to_string().cyan()
    );
    for ((working_dir, command), file) in commands {
        let plan = runner.resolve_test_plan(&file, root);
        println!(
            "   {} ({}) in {}",
            command.dimmed(),
            plan.selection.description().dimmed(),
            working_dir.display()
        );
        let result = runner.run_baseline(&file, root);
        if result.outcome == BaselineOutcome::Passed {
            println!("      ... {}", "passed".green().bold());
            continue;
        }
        return Err(baseline_failure(&result, &file));
    }
    Ok(())
}

fn baseline_failure(result: &BaselineExecutionResult, file: &Path) -> anyhow::Error {
    let diagnostic = if result.diagnostic.trim().is_empty() {
        "no diagnostic output".to_string()
    } else {
        result.diagnostic.clone()
    };
    anyhow::anyhow!(
        "unmutated baseline {:?} for `{}` using `{}`:\n{}",
        result.outcome,
        file.display(),
        result.command,
        diagnostic
    )
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

pub fn select_representative_mutants(
    mut candidates: Vec<AstMutant>,
    max_count: usize,
) -> Vec<AstMutant> {
    let mut grouped: BTreeMap<PathBuf, BTreeMap<(String, String), Vec<AstMutant>>> =
        BTreeMap::new();
    let mut seen = BTreeSet::new();
    candidates.sort_by(mutant_order);
    for mutant in candidates {
        let identity = (
            mutant.file.clone(),
            mutant.start_byte,
            mutant.end_byte,
            mutant.original.clone(),
            mutant.replacement.clone(),
        );
        if seen.insert(identity) {
            grouped
                .entry(mutant.file.clone())
                .or_default()
                .entry((mutant.original.clone(), mutant.replacement.clone()))
                .or_default()
                .push(mutant);
        }
    }
    let selected = round_robin_mutants(grouped, max_count);
    selected
        .into_iter()
        .enumerate()
        .map(|(index, mut mutant)| {
            mutant.id = index + 1;
            mutant
        })
        .collect()
}

fn mutant_order(left: &AstMutant, right: &AstMutant) -> std::cmp::Ordering {
    (
        &left.file,
        left.line,
        left.column,
        left.start_byte,
        left.end_byte,
        &left.original,
        &left.replacement,
        &left.description,
    )
        .cmp(&(
            &right.file,
            right.line,
            right.column,
            right.start_byte,
            right.end_byte,
            &right.original,
            &right.replacement,
            &right.description,
        ))
}

fn round_robin_mutants(
    mut grouped: BTreeMap<PathBuf, BTreeMap<(String, String), Vec<AstMutant>>>,
    max_count: usize,
) -> Vec<AstMutant> {
    let mut selected = Vec::with_capacity(max_count.min(grouped.len()));
    let mut offsets = BTreeMap::new();
    while selected.len() < max_count {
        let mut progressed = false;
        for (file, families) in &mut grouped {
            let offset = offsets.entry(file.clone()).or_insert(0);
            if let Some(mutant) = take_next_family(families, offset) {
                selected.push(mutant);
                progressed = true;
                if selected.len() == max_count {
                    return selected;
                }
            }
        }
        if !progressed {
            break;
        }
    }
    selected
}

fn take_next_family(
    families: &mut BTreeMap<(String, String), Vec<AstMutant>>,
    offset: &mut usize,
) -> Option<AstMutant> {
    let keys = families.keys().cloned().collect::<Vec<_>>();
    if keys.is_empty() {
        return None;
    }
    for attempt in 0..keys.len() {
        let index = (*offset + attempt) % keys.len();
        let key = &keys[index];
        let mutants = families.get_mut(key)?;
        if let Some(mutant) = (!mutants.is_empty()).then(|| mutants.remove(0)) {
            *offset = (index + 1) % keys.len();
            return Some(mutant);
        }
    }
    None
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
        print_outcome(&mut stats, res.outcome);
        results.push(res);
    }

    Ok((results, stats))
}

fn print_outcome(stats: &mut MutationStats, outcome: MutantOutcome) {
    let (label, style) = outcome_label(outcome);
    increment_stats(stats, outcome);
    match style {
        OutcomeStyle::Green => println!("{}", label.green().bold()),
        OutcomeStyle::Red => println!("{}", label.red().bold()),
        OutcomeStyle::Yellow => println!("{}", label.yellow().bold()),
    }
}

#[derive(Clone, Copy)]
enum OutcomeStyle {
    Green,
    Red,
    Yellow,
}

fn outcome_label(outcome: MutantOutcome) -> (&'static str, OutcomeStyle) {
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

fn increment_stats(stats: &mut MutationStats, outcome: MutantOutcome) {
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
