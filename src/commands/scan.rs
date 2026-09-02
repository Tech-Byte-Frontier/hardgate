use super::check::output_report;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::engines::{
    check_file_budgets, AntiGamingScanner, ComplexityAnalyzer, InvariantsChecker,
};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::time::Instant;

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
    if !report.passed {
        std::process::exit(1);
    }
    Ok(())
}
