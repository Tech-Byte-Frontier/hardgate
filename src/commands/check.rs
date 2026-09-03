use super::verify::verify_coverage;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{DiscoverOptions, discover_files_with_exclusions};
use crate::engines::{
    AntiGamingScanner, CloneDetector, ComplexityAnalyzer, DeadCodeAnalyzer, FunctionMetrics,
    InvariantsChecker, OrchestrationEngine, check_file_budgets,
};
use anyhow::Result;
use colored::*;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    pub format: Option<String>,
    pub diff: bool,
    pub all: bool,
    pub dead_code: bool,
    pub coverage_report: Option<String>,
}

pub fn cmd_check(opts: CheckOptions) -> Result<()> {
    let start_time = Instant::now();
    let root = Path::new(".");
    let config = HardgateConfig::load_or_default(None)?;

    let Some((mut report, files, read_results, functions)) = run_static_gate(&config, opts.diff)?
    else {
        print_empty_discovery(opts.diff);
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

    let elapsed = start_time.elapsed().as_millis();
    report.finalize(read_results.len(), functions.len(), elapsed);

    output_report(&report, opts.format.as_deref());
    if !report.passed {
        std::process::exit(1);
    }
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

pub type StaticGateOutcome = Option<(
    GateReport,
    Vec<PathBuf>,
    Vec<(PathBuf, String)>,
    Vec<FunctionMetrics>,
)>;

pub fn run_static_gate(config: &HardgateConfig, diff: bool) -> Result<StaticGateOutcome> {
    let root = Path::new(".");
    let discovery = discover_files_with_exclusions(DiscoverOptions {
        root,
        diff_only: diff,
        exclusions: &config.budgets.files.exclusions.paths,
    })?;

    let files = discovery.files;
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

    let mut all_functions = Vec::new();

    for (path, content) in &read_results {
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
        all_functions.extend(functions);
    }

    (read_results, all_functions)
}

fn print_empty_discovery(diff: bool) {
    if diff {
        println!(
            "{} No git-modified source files detected to check.",
            "✓".green()
        );
    } else {
        println!("{} No matching source files detected.", "⚠️".yellow());
    }
}

pub fn output_report(report: &GateReport, format: Option<&str>) {
    match format {
        Some("agent") => print!("{}", report.render_agent()),
        Some("json") => println!("{}", report.render_json()),
        _ => print!("{}", report.render_terminal()),
    }
}
