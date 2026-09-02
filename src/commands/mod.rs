use crate::config::{HardgateConfig, Preset};
use crate::diagnostics::GateReport;
use crate::discovery::{discover_files, DiscoverOptions};
use crate::engines::{
    check_file_budgets, AntiGamingScanner, CloneDetector, ComplexityAnalyzer, CoverageScorer,
    FunctionMetrics, InvariantsChecker, MutationGatekeeper,
};
use anyhow::{Context, Result};
use colored::*;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn cmd_init(preset_str: &str) -> Result<()> {
    let target = Path::new("hardgate.toml");
    if target.exists() {
        println!("{} `hardgate.toml` already exists in this directory.", "⚠️".yellow());
        return Ok(());
    }

    let preset = match preset_str.to_lowercase().as_str() {
        "balanced" => Preset::Balanced,
        "legacy-migration" => Preset::LegacyMigration,
        "custom" => Preset::Custom,
        _ => Preset::StrictAgent,
    };

    let toml_content = HardgateConfig::generate_toml_template(preset);
    fs::write(target, toml_content)?;

    println!(
        "{} Initialized {} with preset [{}]",
        "✓".green(),
        "hardgate.toml".bold(),
        format!("{:?}", preset).bold()
    );
    Ok(())
}

pub fn cmd_check(format: Option<&str>, diff: bool) -> Result<()> {
    let start_time = Instant::now();
    let config = HardgateConfig::load_or_default(None)?;

    let Some((mut report, file_count, functions)) = run_static_gate(&config, diff)? else {
        print_empty_discovery(diff);
        return Ok(());
    };

    let elapsed = start_time.elapsed().as_millis();
    report.finalize(file_count, functions.len(), elapsed);

    output_report(&report, format);
    exit_if_failed(&report);
    Ok(())
}

pub fn cmd_scan(file_path: &Path, format: Option<&str>) -> Result<()> {
    let start_time = Instant::now();
    let root = Path::new(".");
    let config = HardgateConfig::load_or_default(None)?;

    if !file_path.exists() {
        anyhow::bail!("File not found: {:?}", file_path);
    }

    let content = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {:?}", file_path))?;

    let mut report = GateReport::new(config.gate.name.clone());
    report
        .budget_violations
        .extend(check_file_budgets(file_path, &config.budgets.files, root));

    if config.anti_gaming.disallow_suppressions {
        let scanner = AntiGamingScanner::new(&config.anti_gaming);
        report
            .suppression_violations
            .extend(scanner.scan_content(file_path, &content, root));
    }

    if config.invariants.enforce {
        let invariants = InvariantsChecker::new(&config.invariants.rules);
        report
            .invariant_violations
            .extend(invariants.check_file(file_path, &content, root));
    }

    let mut analyzer = ComplexityAnalyzer::new();
    let functions = analyzer.analyze_file(file_path, &content, root);
    report
        .complexity_violations
        .extend(ComplexityAnalyzer::check_violations(
            &functions,
            &config.budgets.functions,
        ));

    let elapsed = start_time.elapsed().as_millis();
    report.finalize(1, functions.len(), elapsed);

    output_report(&report, format);
    exit_if_failed(&report);
    Ok(())
}

pub fn cmd_verify(
    coverage_report: Option<String>,
    mutation_report: Option<String>,
    format: Option<&str>,
) -> Result<()> {
    let start_time = Instant::now();
    let config = HardgateConfig::load_or_default(None)?;

    let Some((mut report, file_count, functions)) = run_static_gate(&config, false)? else {
        print_empty_discovery(false);
        return Ok(());
    };

    verify_coverage(&config, coverage_report, &functions, &mut report);
    verify_mutation(&config, mutation_report, &mut report);

    let elapsed = start_time.elapsed().as_millis();
    report.finalize(file_count, functions.len(), elapsed);

    output_report(&report, format);
    exit_if_failed(&report);
    Ok(())
}

fn run_static_gate(
    config: &HardgateConfig,
    diff: bool,
) -> Result<Option<(GateReport, usize, Vec<FunctionMetrics>)>> {
    let root = Path::new(".");
    let files = discover_files(DiscoverOptions {
        root,
        diff_only: diff,
        exclusions: &config.budgets.files.exclusions.paths,
    })?;

    if files.is_empty() {
        return Ok(None);
    }

    let mut report = GateReport::new(config.gate.name.clone());
    let (read_results, all_functions) = run_file_analysis(&files, config, root, &mut report);

    if config.clones.enabled && read_results.len() > 1 {
        let detector = CloneDetector::new(&config.clones);
        report.clone_violations.extend(detector.detect_clones(&read_results, root));
    }

    Ok(Some((report, read_results.len(), all_functions)))
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
            fs::read_to_string(path).ok().map(|content| (path.clone(), content))
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

fn verify_coverage(
    config: &HardgateConfig,
    cli_report: Option<String>,
    functions: &[FunctionMetrics],
    report: &mut GateReport,
) {
    let cov_path = cli_report.or_else(|| config.coverage.report.clone());
    let Some(ref path_str) = cov_path else { return };
    let p = Path::new(path_str);
    if !p.exists() {
        return;
    }
    let scorer = CoverageScorer::new(&config.coverage);
    if let Ok(cov_map) = scorer.parse_lcov(p) {
        let cov_violations = scorer.evaluate(&cov_map, functions, Path::new("."));
        report.coverage_violations.extend(cov_violations);
    }
}

fn verify_mutation(
    config: &HardgateConfig,
    cli_report: Option<String>,
    report: &mut GateReport,
) {
    let mut_reports = cli_report
        .map(|r| vec![r])
        .or_else(|| config.mutation.reports.clone());

    let Some(reports) = mut_reports else { return };
    let gatekeeper = MutationGatekeeper::new(&config.mutation);
    for r_str in reports {
        let p = Path::new(&r_str);
        if p.exists() {
            if let Ok(m_violations) = gatekeeper.evaluate_report(p) {
                report.mutation_violations.extend(m_violations);
            }
        }
    }
}

fn print_empty_discovery(diff: bool) {
    if diff {
        println!("{} No git-modified source files detected to check.", "✓".green());
    } else {
        println!("{} No matching source files detected.", "⚠️".yellow());
    }
}

fn exit_if_failed(report: &GateReport) {
    if !report.passed {
        std::process::exit(1);
    }
}

fn output_report(report: &GateReport, format: Option<&str>) {
    match format {
        Some("agent") => print!("{}", report.render_agent()),
        Some("json") => println!("{}", report.render_json()),
        _ => print!("{}", report.render_terminal()),
    }
}
