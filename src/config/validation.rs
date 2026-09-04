use anyhow::{Context, Result, bail};

use super::roles::{ensure_positive, ensure_positive_float};
use super::{
    CloneConfig, CoverageConfig, FileBudgets, FunctionBudgets, GateConfig, HardgateConfig,
    InvariantsConfig, MutationConfig, OrchestrationConfig,
};

pub(super) fn validate(config: &HardgateConfig) -> Result<()> {
    validate_gate(&config.gate)?;
    validate_file_budgets(&config.budgets.files)?;
    validate_function_budgets(&config.budgets.functions)?;
    validate_clones(&config.clones)?;
    validate_coverage(&config.coverage)?;
    validate_mutation(&config.mutation)?;
    validate_orchestration(&config.orchestration)?;
    validate_exclusion_globs(
        &config.budgets.files.exclusions.paths,
        "budgets.files.exclusions.paths",
    )?;
    if let Some(globs) = &config.clones.excludes {
        validate_exclusion_globs(globs, "clones.excludes")?;
    }
    validate_exclusion_globs(
        &config.analysis.dead_code.exclude,
        "analysis.dead_code.exclude",
    )?;
    validate_invariants(&config.invariants)?;
    config.roles.validate()?;
    config.classification.validate()?;
    config.generated.validate(config.gate.strict)?;
    config.legacy.validate()?;
    Ok(())
}

fn validate_gate(gate: &GateConfig) -> Result<()> {
    if gate.name.trim().is_empty() {
        bail!("gate.name must not be empty");
    }
    Ok(())
}

fn validate_file_budgets(files: &FileBudgets) -> Result<()> {
    if let Some(value) = files.max_bytes {
        ensure_positive(value, "budgets.files.max_bytes")?;
    }
    for (extension, value) in &files.max_lines {
        ensure_positive(*value, &format!("budgets.files.max_lines.{extension}"))?;
    }
    Ok(())
}

fn validate_function_budgets(functions: &FunctionBudgets) -> Result<()> {
    for (value, field) in [
        (functions.max_cyclomatic.map(u64::from), "max_cyclomatic"),
        (functions.max_cognitive.map(u64::from), "max_cognitive"),
        (
            functions.max_parameters.map(|value| value as u64),
            "max_parameters",
        ),
        (functions.max_lines.map(|value| value as u64), "max_lines"),
        (
            functions.max_statements.map(|value| value as u64),
            "max_statements",
        ),
        (
            functions.max_nesting_depth.map(|value| value as u64),
            "max_nesting_depth",
        ),
    ] {
        if let Some(value) = value {
            ensure_positive(value, &format!("budgets.functions.{field}"))?;
        }
    }
    for (value, field) in [
        (
            functions.max_halstead_difficulty,
            "budgets.functions.max_halstead_difficulty",
        ),
        (functions.max_abc, "budgets.functions.max_abc"),
    ] {
        if let Some(value) = value {
            ensure_positive_float(value, field)?;
        }
    }
    Ok(())
}

fn validate_clones(clones: &CloneConfig) -> Result<()> {
    ensure_positive(clones.min_lines, "clones.min_lines")?;
    ensure_positive(clones.min_tokens, "clones.min_tokens")?;
    Ok(())
}

fn validate_coverage(coverage: &CoverageConfig) -> Result<()> {
    for (field, value) in [
        ("coverage.min_line_percent", coverage.min_line_percent),
        (
            "coverage.min_function_percent",
            coverage.min_function_percent,
        ),
        ("coverage.min_branch_percent", coverage.min_branch_percent),
    ] {
        if let Some(value) = value {
            ensure_percentage(value, field)?;
        }
    }
    if let Some(value) = coverage.max_crap_score {
        if !value.is_finite() || value < 0.0 {
            bail!("coverage.max_crap_score must be finite and non-negative");
        }
    }
    Ok(())
}

fn validate_mutation(mutation: &MutationConfig) -> Result<()> {
    if mutation.enabled && !mutation.reject_timeouts {
        bail!(
            "mutation.reject_timeouts=false cannot weaken an enabled mutation gate; timeouts are blocking"
        );
    }
    if let Some(value) = mutation.min_score {
        ensure_percentage(value, "mutation.min_score")?;
    }
    if let Some(value) = mutation.timeout_secs {
        ensure_positive(value, "mutation.timeout_secs")?;
    }
    if let Some(value) = mutation.max_mutants {
        ensure_positive(value, "mutation.max_mutants")?;
    }
    Ok(())
}

fn validate_orchestration(orchestration: &OrchestrationConfig) -> Result<()> {
    if let Some(value) = orchestration.timeout_secs {
        ensure_positive(value, "orchestration.timeout_secs")?;
    }
    Ok(())
}

fn validate_invariants(invariants: &InvariantsConfig) -> Result<()> {
    for (index, rule) in invariants.rules.iter().enumerate() {
        validate_glob(&rule.from, &format!("invariants.rules[{index}].from"))?;
        if let Some(globs) = &rule.exclude {
            validate_exclusion_globs(globs, &format!("invariants.rules[{index}].exclude"))?;
        }
    }
    Ok(())
}

fn validate_exclusion_globs(globs: &[String], field: &str) -> Result<()> {
    for (index, glob) in globs.iter().enumerate() {
        validate_glob(glob, &format!("{field}[{index}]"))?;
    }
    Ok(())
}

fn validate_glob(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not contain an empty glob");
    }
    globset::Glob::new(value)
        .map(|_| ())
        .with_context(|| format!("Invalid glob in {field}: `{value}`"))
}

fn ensure_percentage(value: f64, field: &str) -> Result<()> {
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        bail!("{field} must be a finite percentage between 0 and 100");
    }
    Ok(())
}
