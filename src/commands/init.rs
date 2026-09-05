mod detect;
mod manifest;
mod mixed;
mod reference;
mod render;
mod tooling;

use anyhow::{Context, Result};
use detect::{Detection, ReferenceStatus};
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, ErrorKind, Write};
use std::path::Path;

use crate::config::{HardgateConfig, Preset};

const LEGACY_REFERENCE: &str = "origin/main";

/// Inputs for deterministic policy initialization.
///
/// Formatter and linter values are complete command strings. They are written
/// as TOML strings and are never executed during initialization.
#[derive(Debug, Clone)]
pub struct InitOptions {
    pub preset: String,
    pub preview: bool,
    pub full: bool,
    pub format_check: Option<String>,
    pub format: Option<String>,
    pub lint: Option<String>,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            preset: "balanced".to_string(),
            preview: false,
            full: false,
            format_check: None,
            format: None,
            lint: None,
        }
    }
}

/// Write a hardgate.toml for a preset, preserving the historical API.
pub fn cmd_init(preset_str: &str) -> Result<()> {
    cmd_init_with_options(InitOptions {
        preset: preset_str.to_string(),
        ..InitOptions::default()
    })
}

/// Detect project metadata and initialize a policy without running project
/// commands or replacing an existing directory entry.
pub fn cmd_init_with_options(options: InitOptions) -> Result<()> {
    let root = std::env::current_dir().context("cannot resolve the current directory")?;
    init_at(&root, &options)
}

fn init_at(root: &Path, options: &InitOptions) -> Result<()> {
    let target = root.join("hardgate.toml");
    if !options.preview && entry_exists(&target)? {
        write_stderr("warning: hardgate.toml already exists in this directory.\n")?;
        return Ok(());
    }

    let (preset, fallback) = parse_preset(&options.preset);
    let detection = detect::detect_project(root);
    let config = render::effective_config(preset, &detection, options);
    let reference_status = if preset == Preset::LegacyMigration {
        detect::legacy_reference_status(root, LEGACY_REFERENCE)
    } else {
        ReferenceStatus::NotApplicable
    };
    let setup = SetupContext {
        root,
        preset,
        config: &config,
        detection: &detection,
        reference_status,
    };
    let missing_setup = missing_setup(&setup);
    let content = render::render(render::RenderInput {
        config: &config,
        preset,
        detection: &detection,
        missing_setup: &missing_setup,
        reference_status,
        full: options.full,
    });
    let summary = completion_summary(&SummaryContext {
        preset,
        config: &config,
        detection: &detection,
        missing: &missing_setup,
        reference_status,
        preview: options.preview,
    });

    let mut status = String::new();
    if fallback {
        status.push_str("warning: unknown preset; using strict-agent.\n");
    }
    if options.preview {
        write_stdout(&content)?;
        status.push_str("preview: no file was written.\n");
    } else {
        match write_new(&target, content.as_bytes())? {
            true => status.push_str("created: hardgate.toml\n"),
            false => {
                status.push_str("warning: hardgate.toml already exists in this directory.\n");
                return write_stderr(&status);
            }
        }
    }
    status.push_str(&summary);
    write_stderr(&status)
}

fn parse_preset(value: &str) -> (Preset, bool) {
    match value.to_ascii_lowercase().as_str() {
        "strict-agent" => (Preset::StrictAgent, false),
        "balanced" => (Preset::Balanced, false),
        "legacy-migration" => (Preset::LegacyMigration, false),
        "custom" => (Preset::Custom, false),
        _ => (Preset::StrictAgent, true),
    }
}

fn entry_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("cannot inspect {}", path.display())),
    }
}

fn write_new(path: &Path, content: &[u8]) -> Result<bool> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| format!("cannot create {}", path.display()));
        }
    };
    file.write_all(content)
        .with_context(|| format!("cannot write {}", path.display()))?;
    file.flush()
        .with_context(|| format!("cannot flush {}", path.display()))?;
    Ok(true)
}

struct SetupContext<'a> {
    root: &'a Path,
    preset: Preset,
    config: &'a HardgateConfig,
    detection: &'a Detection,
    reference_status: ReferenceStatus,
}

fn missing_setup(context: &SetupContext<'_>) -> Vec<String> {
    let mut missing = context
        .detection
        .missing_setup
        .iter()
        .filter(|message| !resolved_by_override(message, context.config))
        .cloned()
        .collect();
    append_coverage_setup(&mut missing, context.root, context.config);
    append_mutation_setup(&mut missing, context.root, context.config);
    if context.preset == Preset::LegacyMigration
        && !matches!(context.reference_status, ReferenceStatus::Available)
    {
        missing.push("legacy reference origin/main is not currently usable".to_string());
    }
    deduplicate(missing)
}

fn resolved_by_override(message: &str, config: &HardgateConfig) -> bool {
    let message = message.to_ascii_lowercase();
    let formatter_override =
        config.orchestration.format_check.is_some() || config.orchestration.format.is_some();
    let linter_override = config.orchestration.lint.is_some();
    (formatter_override
        && (message.contains("formatter")
            || message.contains("prettier")
            || message.contains("biome")))
        || (linter_override
            && (message.contains("linter")
                || message.contains("eslint")
                || message.contains("oxlint")))
}

fn append_coverage_setup(missing: &mut Vec<String>, root: &Path, config: &HardgateConfig) {
    if !config.coverage.enabled {
        return;
    }
    match &config.coverage.report {
        Some(report) if !root.join(report).is_file() => {
            missing.push(format!("coverage report {report} is not present yet"));
        }
        None => missing.push("configure coverage.report and generate an LCOV report".to_string()),
        _ => {}
    }
}

fn append_mutation_setup(missing: &mut Vec<String>, root: &Path, config: &HardgateConfig) {
    if !config.mutation.enabled {
        return;
    }
    match &config.mutation.reports {
        Some(reports) if reports.is_empty() => {
            missing.push("configure mutation.reports with a generated report".to_string());
        }
        Some(reports) => append_missing_reports(missing, root, reports),
        None => missing.push("configure mutation.reports with a generated report".to_string()),
    }
}

fn append_missing_reports(missing: &mut Vec<String>, root: &Path, reports: &[String]) {
    for report in reports {
        if !root.join(report).is_file() {
            missing.push(format!("mutation report {report} is not present yet"));
        }
    }
}

fn deduplicate(values: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for value in values {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    unique
}

struct SummaryContext<'a> {
    preset: Preset,
    config: &'a HardgateConfig,
    detection: &'a Detection,
    missing: &'a [String],
    reference_status: ReferenceStatus,
    preview: bool,
}

fn completion_summary(context: &SummaryContext<'_>) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "summary: preset={} project={}{}.\n",
        preset_name(context.preset),
        context.detection.ecosystem.label(),
        if context.preview { " (preview)" } else { "" }
    ));
    output.push_str(&format!(
        "enabled engines: {}.\n",
        enabled_engines(context.config)
    ));
    if context.missing.is_empty() {
        output.push_str("missing setup: none detected.\n");
    } else {
        output.push_str("missing setup:\n");
        for item in context.missing {
            output.push_str(&format!("  - {item}\n"));
        }
    }
    if context.preset == Preset::StrictAgent
        && context.config.coverage.enabled
        && context.config.mutation.enabled
    {
        output.push_str(
            "strict evidence: hardgate check also requires generated LCOV and mutation reports.\n",
        );
    }
    if context.preset == Preset::LegacyMigration {
        output.push_str(&format!(
            "legacy reference: {}.\n",
            reference_label(context.reference_status)
        ));
    }
    output.push_str(&format!(
        "next: {}.\n",
        next_command(
            context.preset,
            context.config,
            context.missing,
            context.reference_status,
        )
    ));
    output
}

fn enabled_engines(config: &HardgateConfig) -> String {
    let mut engines = vec![
        "structural budgets".to_string(),
        "anti-gaming".to_string(),
        "invariants".to_string(),
        "clone analysis".to_string(),
    ];
    if config.coverage.enabled {
        engines.push("coverage evidence".to_string());
    }
    if config.mutation.enabled {
        engines.push("mutation evidence".to_string());
    }
    if config.legacy.ratchet {
        engines.push("legacy ratchet".to_string());
    }
    if config.orchestration.format_check.is_some() || config.orchestration.format.is_some() {
        engines.push("formatter".to_string());
    }
    if config.orchestration.lint.is_some() {
        engines.push("linter".to_string());
    }
    if config.orchestration.test_cmd.is_some() {
        engines.push("tests".to_string());
    }
    engines.join(", ")
}

fn next_command(
    preset: Preset,
    config: &HardgateConfig,
    missing: &[String],
    reference_status: ReferenceStatus,
) -> &'static str {
    if preset == Preset::LegacyMigration && !matches!(reference_status, ReferenceStatus::Available)
    {
        return "git fetch origin main";
    }
    if preset == Preset::StrictAgent
        && config.coverage.enabled
        && missing
            .iter()
            .any(|item| item.contains("coverage") || item.contains("mutation"))
    {
        return "hardgate config";
    }
    if config.orchestration.format_check.is_some()
        || config.orchestration.lint.is_some()
        || config.orchestration.test_cmd.is_some()
    {
        "hardgate check --all"
    } else {
        "hardgate check"
    }
}

fn reference_label(status: ReferenceStatus) -> &'static str {
    match status {
        ReferenceStatus::NotApplicable => "not checked",
        ReferenceStatus::Available => "available",
        ReferenceStatus::Missing => "missing",
        ReferenceStatus::Unknown => "unknown",
    }
}

pub(crate) fn preset_name(preset: Preset) -> &'static str {
    match preset {
        Preset::StrictAgent => "strict-agent",
        Preset::Balanced => "balanced",
        Preset::LegacyMigration => "legacy-migration",
        Preset::Custom => "custom",
    }
}

fn write_stdout(content: &str) -> Result<()> {
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    writer.write_all(content.as_bytes())?;
    writer.flush()?;
    Ok(())
}

fn write_stderr(content: &str) -> Result<()> {
    let stderr = io::stderr();
    let mut writer = BufWriter::new(stderr.lock());
    writer.write_all(content.as_bytes())?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
#[path = "init/init_tests.rs"]
mod tests;
