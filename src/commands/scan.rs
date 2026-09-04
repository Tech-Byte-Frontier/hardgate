use super::check::{Emission, OutputOptions, emit_gate_report};
use super::static_gate::{AnalyzeInput, analyze_file_content};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::engines::{AntiGamingScanner, InvariantsChecker};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Inspect one file's AST metrics, suppressions, and budgets, then render
/// and exit non-zero when violations are found.
pub fn cmd_scan(file_path: &Path, opts: OutputOptions) -> Result<()> {
    let start_time = std::time::Instant::now();
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

    emit_gate_report(
        &mut report,
        Emission {
            read_len: 1,
            fn_len: functions.len(),
            elapsed: start_time.elapsed().as_millis(),
            opts: &opts,
        },
    );
    Ok(())
}

/// Backwards-compatible helper for callers passing `format: Option<&str>`.
/// Prefer [`cmd_scan`] with [`OutputOptions`] for the full flag matrix.
pub fn cmd_scan_with_format(file_path: &Path, format: Option<&str>) -> Result<()> {
    cmd_scan(
        file_path,
        OutputOptions {
            format: format.map(|s| s.to_string()),
            ..Default::default()
        },
    )
}
