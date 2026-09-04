use super::role_policy::{apply_dead_code_findings, classify_file};
use super::static_gate::run_static_gate_scoped;
use super::verify::{verify_coverage, verify_mutation};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::FileRole;
use crate::engines::{DeadCodeAnalyzer, OrchestrationEngine};
use anyhow::Result;
use colored::*;
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

    let Some((mut report, _files, read_results, functions)) =
        run_static_gate_scoped(&config, opts.diff, &opts.paths)?
    else {
        print_empty_discovery(opts.diff, !opts.paths.is_empty());
        return Ok(());
    };

    if opts.dead_code || config.analysis.dead_code.enabled {
        run_dead_code_analysis(&config, &read_results, root, &mut report)?;
    }

    if config.coverage.enabled {
        verify_coverage(
            &config,
            find_coverage_report(&config, opts.coverage_report.clone()),
            &functions,
            &mut report,
        );
    }

    if config.mutation.enabled {
        verify_mutation(&config, None, &mut report);
    }

    if opts.all {
        let orch = OrchestrationEngine::new(&config.orchestration);
        let (_res, violations) = orch.run_all_checks(root);
        report.orchestration_violations.extend(violations);
    }

    report.advisories.push(check_scope_advisory(&config, &opts));

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

fn run_dead_code_analysis(
    config: &HardgateConfig,
    read_results: &[(PathBuf, String)],
    root: &Path,
    report: &mut GateReport,
) -> Result<()> {
    let mut graph_files = Vec::new();
    let mut graph_contents = Vec::new();
    let mut graph_roles = Vec::new();
    for (path, content) in read_results {
        let classified = classify_file(path, config)?;
        if !classified.ast_supported
            || !matches!(
                classified.role,
                FileRole::Source | FileRole::Test | FileRole::Generated | FileRole::Fixture
            )
        {
            continue;
        }
        graph_files.push(path.clone());
        graph_contents.push((path.clone(), content.clone()));
        graph_roles.push(classified);
    }
    let analyzer = DeadCodeAnalyzer::new(&config.analysis.dead_code);
    let findings = analyzer.analyze(&graph_files, &graph_contents, root);
    for finding in findings {
        let role = graph_roles
            .iter()
            .find(|file| relative_path(&file.path, root) == finding.file)
            .map(|file| file.role)
            .unwrap_or(FileRole::Unknown);
        apply_dead_code_findings(report, config, role, vec![finding]);
    }
    Ok(())
}

fn relative_path<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

fn find_coverage_report(config: &HardgateConfig, cli_report: Option<String>) -> Option<String> {
    if cli_report.is_some() {
        return cli_report;
    }
    if config.coverage.report.is_some() {
        return config.coverage.report.clone();
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

fn check_scope_advisory(config: &HardgateConfig, opts: &CheckOptions) -> String {
    let mut omitted = Vec::new();
    if !opts.all {
        omitted.push("configured formatter/linter/test commands");
    }
    if !opts.dead_code && !config.analysis.dead_code.enabled {
        omitted.push("dead-code analysis");
    }
    if !config.coverage.enabled {
        omitted.push("coverage evidence (disabled by policy)");
    }
    if !config.mutation.enabled {
        omitted.push("mutation evidence (disabled by policy)");
    }
    if omitted.is_empty() {
        "This check evaluated every configured report and static/orchestration engine; native mutation execution remains a separate `hardgate mutate` command."
            .to_string()
    } else {
        format!(
            "This is a partial gate; omitted {}. Use `check --all --dead-code`, `verify`, and an enabled `mutate` policy for complete evidence.",
            omitted.join(", ")
        )
    }
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

#[cfg(test)]
mod tests {
    use super::run_dead_code_analysis;
    use crate::config::HardgateConfig;
    use crate::diagnostics::GateReport;
    use std::path::Path;
    use std::path::PathBuf;

    #[test]
    fn dead_code_graph_ignores_config_but_reports_unreferenced_source() {
        let config = HardgateConfig::default();
        let contents = vec![
            (PathBuf::from("src/unused.rs"), "fn unused() {}".to_string()),
            (PathBuf::from("package.json"), "{}".to_string()),
            (PathBuf::from("Cargo.toml"), "[package]".to_string()),
        ];
        let mut report = GateReport::new("test".to_string());
        run_dead_code_analysis(&config, &contents, Path::new("."), &mut report).unwrap();
        assert!(
            report
                .dead_code_violations
                .iter()
                .any(|finding| finding.file.as_path() == Path::new("src/unused.rs"))
        );
        assert!(
            report
                .dead_code_violations
                .iter()
                .all(|finding| finding.file.as_path() != Path::new("package.json"))
        );
        assert!(
            report
                .dead_code_violations
                .iter()
                .all(|finding| finding.file.as_path() != Path::new("Cargo.toml"))
        );
    }
}
