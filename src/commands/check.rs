use super::verify::verify_coverage;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{DiscoverOptions, discover_files_with_exclusions, filter_files_by_paths};
use crate::engines::{
    AntiGamingScanner, BudgetViolation, CloneDetector, ComplexityAnalyzer, ComplexityViolation,
    DeadCodeAnalyzer, FunctionMetrics, InvariantViolation, InvariantsChecker, OrchestrationEngine,
    SuppressionViolation, check_file_budgets,
};
use anyhow::Result;
use colored::*;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// CLI options for `hardgate check`, including output modes and path scoping.
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    pub format: Option<String>,
    pub diff: bool,
    pub all: bool,
    pub dead_code: bool,
    pub coverage_report: Option<String>,
    pub json: bool,
    pub compact: bool,
    pub no_snippets: bool,
    pub summary: bool,
    pub paths: Vec<PathBuf>,
}

/// Resolved output mode shared by `check`, `scan`, and `verify`.
#[derive(Debug, Clone, Default)]
pub struct OutputOptions {
    pub format: Option<String>,
    pub json: bool,
    pub compact: bool,
    pub no_snippets: bool,
    pub summary: bool,
}

impl OutputOptions {
    /// True for `--json` or `--format json`.
    pub fn is_json(&self) -> bool {
        self.json || matches!(self.format.as_deref(), Some("json"))
    }
    /// True for `--summary` or `--format summary`.
    pub fn is_summary(&self) -> bool {
        self.summary || matches!(self.format.as_deref(), Some("summary"))
    }
    /// True for `--compact`, `--no-snippets`, or `--format compact`.
    pub fn is_compact(&self) -> bool {
        self.compact || self.no_snippets || matches!(self.format.as_deref(), Some("compact"))
    }
}

/// Run the fast deterministic static gate: budgets, suppressions,
/// invariants, complexity, clones, and optional dead-code, coverage, and
/// orchestration checks. Exits non-zero when violations are found.
pub fn cmd_check(opts: CheckOptions) -> Result<()> {
    let start_time = Instant::now();
    let root = Path::new(".");
    let config = HardgateConfig::load_or_default(None)?;

    let Some((mut report, files, read_results, functions)) =
        run_static_gate_scoped(&config, opts.diff, &opts.paths)?
    else {
        print_empty_discovery(opts.diff, !opts.paths.is_empty());
        return Ok(());
    };

    if opts.dead_code || config.analysis.dead_code.enabled {
        let analyzer = DeadCodeAnalyzer::new(&config.analysis.dead_code);
        report
            .dead_code_violations
            .extend(analyzer.analyze(&files, &read_results, root));
    }

    if let Some(cov_path) = find_coverage_report(&config, opts.coverage_report) {
        verify_coverage(&config, Some(cov_path), &functions, &mut report);
    }

    if opts.all {
        let orch = OrchestrationEngine::new(&config.orchestration);
        let (_res, violations) = orch.run_all_checks(root);
        report.orchestration_violations.extend(violations);
    }

    // `check` is static-only by design (sub-second): it never executes
    // mutants, dead-code graphs, or external formatters/linters. One
    // advisory names the full gate so green is never mistaken for fully
    // gated; humans and LLM agents get the exact next commands.
    report.advisories.push(
        "Static check excludes live mutation testing, dead-code analysis, and formatter/linter orchestration. Full gate: `hardgate check --all --dead-code` plus `hardgate mutate --diff`."
            .to_string(),
    );

    let elapsed = start_time.elapsed().as_millis();
    emit_gate_report(
        &mut report,
        Emission {
            read_len: read_results.len(),
            fn_len: functions.len(),
            elapsed,
            opts: &OutputOptions {
                format: opts.format,
                json: opts.json,
                compact: opts.compact,
                no_snippets: opts.no_snippets,
                summary: opts.summary,
            },
        },
    );
    Ok(())
}

fn find_coverage_report(config: &HardgateConfig, cli_report: Option<String>) -> Option<String> {
    if cli_report.is_some() {
        return cli_report;
    }
    if let Some(ref r) = config.coverage.report {
        if Path::new(r).exists() {
            return Some(r.clone());
        }
    }
    let candidates = [
        "coverage/lcov.info",
        "lcov.info",
        "target/llvm-cov/lcov.info",
    ];
    for cand in &candidates {
        if Path::new(cand).exists() {
            return Some(cand.to_string());
        }
    }
    None
}

/// Artifacts of one static-gate run: the report plus the discovered files,
/// their contents, and per-function metrics for downstream gates.
pub type StaticGateOutcome = Option<(
    GateReport,
    Vec<PathBuf>,
    Vec<(PathBuf, String)>,
    Vec<FunctionMetrics>,
)>;

/// Run the static gate over the whole discovered tree (see
/// [`run_static_gate_scoped`] for path-filtered runs).
pub fn run_static_gate(config: &HardgateConfig, diff: bool) -> Result<StaticGateOutcome> {
    run_static_gate_scoped(config, diff, &[])
}

/// Run the static gate, optionally scoped to `paths` (files or directories).
/// Missing filter paths are an error; an empty discovery yields `None`.
pub fn run_static_gate_scoped(
    config: &HardgateConfig,
    diff: bool,
    paths: &[PathBuf],
) -> Result<StaticGateOutcome> {
    let root = Path::new(".");
    let discovery = discover_files_with_exclusions(DiscoverOptions {
        root,
        diff_only: diff,
        exclusions: &config.budgets.files.exclusions.paths,
    })?;

    let files = filter_files_by_paths(discovery.files, paths, root)?;
    let excluded_files = discovery.excluded_files;

    if files.is_empty() {
        return Ok(None);
    }

    let mut report = GateReport::new(config.gate.name.clone());

    if !excluded_files.is_empty() {
        let count = excluded_files.len();
        let noun = if count == 1 { "file" } else { "files" };
        report.advisories.push(format!(
            "{} {} excluded from file budget checks via hardgate.toml.",
            count, noun
        ));
    }

    let (read_results, all_functions) = run_file_analysis(&files, config, root, &mut report);

    if config.clones.enabled {
        let detector = CloneDetector::new(&config.clones);
        let clone_excluded = detector.count_excluded_files(&read_results, root);
        if clone_excluded > 0 {
            let noun = if clone_excluded == 1 { "file" } else { "files" };
            report.advisories.push(format!(
                "{} {} excluded from clone detection via hardgate.toml.",
                clone_excluded, noun
            ));
        }

        if read_results.len() > 1 {
            report
                .clone_violations
                .extend(detector.detect_clones(&read_results, root));
        }
    }

    Ok(Some((report, files, read_results, all_functions)))
}

fn run_file_analysis(
    files: &[PathBuf],
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) -> (Vec<(PathBuf, String)>, Vec<FunctionMetrics>) {
    let anti_gaming = AntiGamingScanner::new(&config.anti_gaming);
    let invariants = InvariantsChecker::new(&config.invariants.rules);

    let read_results: Vec<(PathBuf, String)> = files
        .par_iter()
        .filter_map(|path| {
            fs::read_to_string(path)
                .ok()
                .map(|content| (path.clone(), content))
        })
        .collect();

    // Parallelize per-file analysis (budgets + suppressions + invariants +
    // complexity) — previously serial. Collect then merge serially into report
    // to avoid lock contention.
    type FileAnalysis = (
        Vec<BudgetViolation>,
        Vec<SuppressionViolation>,
        Vec<InvariantViolation>,
        Vec<FunctionMetrics>,
        Vec<ComplexityViolation>,
    );
    let analyzed: Vec<FileAnalysis> = read_results
        .par_iter()
        .map(|(path, content)| {
            let budgets = check_file_budgets(path, &config.budgets.files, root);
            let suppressions = if config.anti_gaming.disallow_suppressions {
                anti_gaming.scan_content(path, content, root)
            } else {
                Vec::new()
            };
            let inv = if config.invariants.enforce {
                invariants.check_file(path, content, root)
            } else {
                Vec::new()
            };
            let mut analyzer = ComplexityAnalyzer::new();
            let functions = analyzer.analyze_file(path, content, root);
            let violations =
                ComplexityAnalyzer::check_violations(&functions, &config.budgets.functions);
            (budgets, suppressions, inv, functions, violations)
        })
        .collect();

    let mut all_functions = Vec::new();
    for (budgets, suppressions, inv, functions, violations) in analyzed {
        report.budget_violations.extend(budgets);
        report.suppression_violations.extend(suppressions);
        report.invariant_violations.extend(inv);
        report.complexity_violations.extend(violations);
        all_functions.extend(functions);
    }

    (read_results, all_functions)
}

/// Print the "nothing to check" note, distinguishing scoped runs from diffs.
pub fn print_empty_discovery(diff: bool, scoped: bool) {
    if scoped {
        println!(
            "{} no matching source files detected for the given path(s).",
            "warning:".yellow().bold()
        );
    } else if diff {
        println!(
            "{} no git-modified source files detected to check.",
            "note:".green().bold()
        );
    } else {
        println!(
            "{} no matching source files detected.",
            "warning:".yellow().bold()
        );
    }
}

/// Render `report` with a legacy `format` name (`agent`, `json`, terminal).
/// Prefer [`output_report_with_opts`] for the full flag matrix.
pub fn output_report(report: &GateReport, format: Option<&str>) {
    output_report_with_opts(
        report,
        &OutputOptions {
            format: format.map(|s| s.to_string()),
            json: false,
            compact: false,
            no_snippets: false,
            summary: false,
        },
    );
}

/// Render `report` honoring JSON, agent, summary, compact, and terminal modes.
pub fn output_report_with_opts(report: &GateReport, opts: &OutputOptions) {
    if opts.is_json() {
        if opts.is_summary() {
            println!("{}", report.render_summary_json());
        } else {
            println!("{}", report.render_json());
        }
        return;
    }
    match opts.format.as_deref() {
        Some("agent") => print!("{}", report.render_agent()),
        _ if opts.is_summary() => print!("{}", report.render_summary()),
        _ if opts.is_compact() => print!("{}", report.render_compact()),
        _ => print!("{}", report.render_terminal()),
    }
}

/// Finalize counts, render via [`OutputOptions`], and exit non-zero on
/// failure. Shared by `check`, `scan`, and `verify` so the tail of every
/// gate command stays a single call instead of a duplicated clone block.
pub struct Emission<'a> {
    pub read_len: usize,
    pub fn_len: usize,
    pub elapsed: u128,
    pub opts: &'a OutputOptions,
}

/// Finalize `report` from `emission` counts, render it, and exit(1) on failure.
pub fn emit_gate_report(report: &mut GateReport, emission: Emission) {
    report.finalize(emission.read_len, emission.fn_len, emission.elapsed);
    output_report_with_opts(report, emission.opts);
    if !report.passed {
        std::process::exit(1);
    }
}

/// Shared single-file analysis used by `check`, `scan`, and the MCP server.
/// Runs budgets + suppressions + invariants + complexity and appends to `report`.
pub struct AnalyzeInput<'a> {
    pub path: &'a Path,
    pub content: &'a str,
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub anti_gaming: &'a AntiGamingScanner,
    pub invariants: &'a InvariantsChecker,
}

/// Run budgets, suppressions, invariants, and complexity for one file,
/// appending violations to `report` and returning its function metrics.
pub fn analyze_file_content(input: AnalyzeInput, report: &mut GateReport) -> Vec<FunctionMetrics> {
    let AnalyzeInput {
        path,
        content,
        config,
        root,
        anti_gaming,
        invariants,
    } = input;
    report
        .budget_violations
        .extend(check_file_budgets(path, &config.budgets.files, root));

    if config.anti_gaming.disallow_suppressions {
        report
            .suppression_violations
            .extend(anti_gaming.scan_content(path, content, root));
    }

    if config.invariants.enforce {
        report
            .invariant_violations
            .extend(invariants.check_file(path, content, root));
    }

    let mut analyzer = ComplexityAnalyzer::new();
    let functions = analyzer.analyze_file(path, content, root);
    report
        .complexity_violations
        .extend(ComplexityAnalyzer::check_violations(
            &functions,
            &config.budgets.functions,
        ));
    functions
}
