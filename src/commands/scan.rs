use super::check::{AnalyzeInput, analyze_file_content, output_report};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::engines::{AntiGamingScanner, InvariantsChecker};
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
    let scanner = AntiGamingScanner::new(&config.anti_gaming);
    let invariants = InvariantsChecker::new(&config.invariants.rules);
    let functions = analyze_file_content(
        AnalyzeInput {
            path: file_path,
            content: &content,
            config: &config,
            root,
            anti_gaming: &scanner,
            invariants: &invariants,
        },
        &mut report,
    );

    let elapsed = start_time.elapsed().as_millis();
    report.finalize(1, functions.len(), elapsed);

    output_report(&report, format);
    if !report.passed {
        std::process::exit(1);
    }
    Ok(())
}
