use super::check::{output_report, run_static_gate};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::engines::{CoverageScorer, FunctionMetrics, MutationGatekeeper};
use anyhow::Result;
use colored::*;
use std::path::Path;
use std::time::Instant;

pub fn cmd_verify(
    coverage_report: Option<String>,
    mutation_report: Option<String>,
    format: Option<&str>,
) -> Result<()> {
    let start_time = Instant::now();
    let config = HardgateConfig::load_or_default(None)?;

    let Some((mut report, _files, read_results, functions)) = run_static_gate(&config, false)?
    else {
        println!("{} No matching source files detected.", "⚠️".yellow());
        return Ok(());
    };

    verify_coverage(&config, coverage_report, &functions, &mut report);
    verify_mutation(&config, mutation_report, &mut report);

    let elapsed = start_time.elapsed().as_millis();
    report.finalize(read_results.len(), functions.len(), elapsed);

    output_report(&report, format);
    if !report.passed {
        std::process::exit(1);
    }
    Ok(())
}

pub fn verify_coverage(
    config: &HardgateConfig,
    cli_report: Option<String>,
    functions: &[FunctionMetrics],
    report: &mut GateReport,
) {
    let cov_path = cli_report.or_else(|| config.coverage.report.clone());
    let Some(ref path_str) = cov_path else { return };
    let p = Path::new(path_str);
    if !p.exists() {
        report.advisories.push(format!(
            "Coverage report `{}` not found; skipping coverage gate.",
            path_str
        ));
        return;
    }
    let scorer = CoverageScorer::new(&config.coverage);
    match scorer.parse_lcov(p) {
        Ok(cov_map) => {
            let cov_violations = scorer.evaluate(&cov_map, functions, Path::new("."));
            report.coverage_violations.extend(cov_violations);
        }
        Err(e) => {
            report.advisories.push(format!(
                "Failed to parse coverage report `{}`: {}; skipping coverage gate.",
                path_str, e
            ));
        }
    }
}

pub fn verify_mutation(
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
        if !p.exists() {
            report
                .advisories
                .push(format!("Mutation report `{}` not found; skipping.", r_str));
            continue;
        }
        match gatekeeper.evaluate_report(p) {
            Ok(m_violations) => report.mutation_violations.extend(m_violations),
            Err(e) => {
                report.advisories.push(format!(
                    "Failed to parse mutation report `{}`: {}; skipping.",
                    r_str, e
                ));
            }
        }
    }
}
