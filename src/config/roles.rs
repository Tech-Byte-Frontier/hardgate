//! Role-specific policy and classification configuration.
//!
//! The global engine sections remain the source of truth for backwards
//! compatibility.  Every field in [`RolePolicy`] is optional so a project can
//! tighten one role without having to duplicate the complete global budget.

use anyhow::{Result, bail};
use globset::Glob;
use serde::{Deserialize, Serialize};

use crate::discovery::FileRole;

/// Finding severity for a role-specific policy.
///
/// Engines map `error` to a blocking finding, `warning` to an advisory, and
/// `ignore` to an intentionally non-blocking result.  Keeping this as an enum
/// makes misspelled severities fail while loading TOML instead of silently
/// weakening a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    #[default]
    Error,
    Warning,
    Ignore,
}

/// Optional per-role overrides for the static engines.
///
/// `None` means inherit the corresponding global engine budget.  A role is
/// still classified independently when all fields are omitted; the engines
/// decide which inherited budgets apply to that role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RolePolicy {
    pub severity: Option<Severity>,
    pub max_bytes: Option<u64>,
    pub max_lines: Option<usize>,
    pub max_cyclomatic: Option<u32>,
    pub max_cognitive: Option<u32>,
    pub max_halstead_difficulty: Option<f64>,
    pub max_abc: Option<f64>,
    pub max_parameters: Option<usize>,
    #[serde(alias = "max_function_lines")]
    pub max_function_lines: Option<usize>,
    pub max_statements: Option<usize>,
    pub max_nesting_depth: Option<usize>,
    pub clone_enabled: Option<bool>,
    pub clone_min_lines: Option<usize>,
    pub clone_min_tokens: Option<usize>,
    pub mutation_target: Option<bool>,
}

impl RolePolicy {
    /// Overlay explicitly configured fields onto this policy.
    pub fn merge_from(&mut self, overrides: &Self) {
        merge_option(&mut self.severity, &overrides.severity);
        merge_option(&mut self.max_bytes, &overrides.max_bytes);
        merge_option(&mut self.max_lines, &overrides.max_lines);
        merge_option(&mut self.max_cyclomatic, &overrides.max_cyclomatic);
        merge_option(&mut self.max_cognitive, &overrides.max_cognitive);
        merge_option(
            &mut self.max_halstead_difficulty,
            &overrides.max_halstead_difficulty,
        );
        merge_option(&mut self.max_abc, &overrides.max_abc);
        merge_option(&mut self.max_parameters, &overrides.max_parameters);
        merge_option(&mut self.max_function_lines, &overrides.max_function_lines);
        merge_option(&mut self.max_statements, &overrides.max_statements);
        merge_option(&mut self.max_nesting_depth, &overrides.max_nesting_depth);
        merge_option(&mut self.clone_enabled, &overrides.clone_enabled);
        merge_option(&mut self.clone_min_lines, &overrides.clone_min_lines);
        merge_option(&mut self.clone_min_tokens, &overrides.clone_min_tokens);
        merge_option(&mut self.mutation_target, &overrides.mutation_target);
    }

    fn validate(&self, role: FileRole) -> Result<()> {
        let name = role_name(role);
        validate_integer_thresholds(self, name)?;
        validate_float_thresholds(self, name)?;
        if role != FileRole::Source && self.mutation_target == Some(true) {
            bail!(
                "roles.{}.mutation_target may only be true for source",
                role_name(role)
            );
        }
        Ok(())
    }
}

fn validate_integer_thresholds(policy: &RolePolicy, role: &str) -> Result<()> {
    for (value, field) in [
        (policy.max_bytes, "max_bytes"),
        (policy.max_lines.map(|value| value as u64), "max_lines"),
        (policy.max_cyclomatic.map(u64::from), "max_cyclomatic"),
        (policy.max_cognitive.map(u64::from), "max_cognitive"),
        (
            policy.max_parameters.map(|value| value as u64),
            "max_parameters",
        ),
        (
            policy.max_function_lines.map(|value| value as u64),
            "max_function_lines",
        ),
        (
            policy.max_statements.map(|value| value as u64),
            "max_statements",
        ),
        (
            policy.max_nesting_depth.map(|value| value as u64),
            "max_nesting_depth",
        ),
        (
            policy.clone_min_lines.map(|value| value as u64),
            "clone_min_lines",
        ),
        (
            policy.clone_min_tokens.map(|value| value as u64),
            "clone_min_tokens",
        ),
    ] {
        if let Some(value) = value {
            ensure_positive(value, &format!("roles.{role}.{field}"))?;
        }
    }
    Ok(())
}

fn validate_float_thresholds(policy: &RolePolicy, role: &str) -> Result<()> {
    for (value, field) in [
        (policy.max_halstead_difficulty, "max_halstead_difficulty"),
        (policy.max_abc, "max_abc"),
    ] {
        if let Some(value) = value {
            ensure_positive_float(value, &format!("roles.{role}.{field}"))?;
        }
    }
    Ok(())
}

/// Independently configurable policy for each first-class repository role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RolePoliciesConfig {
    #[serde(default)]
    pub source: RolePolicy,
    #[serde(default)]
    pub test: RolePolicy,
    #[serde(default)]
    pub generated: RolePolicy,
    #[serde(default)]
    pub fixture: RolePolicy,
    #[serde(default)]
    pub migration: RolePolicy,
}

impl RolePoliciesConfig {
    /// Defaults used by all non-custom presets.
    pub fn for_preset(strict: bool) -> Self {
        let (test_lines, test_tokens, fixture_lines, fixture_tokens) = if strict {
            (8, 80, 20, 200)
        } else {
            (12, 120, 30, 300)
        };
        Self {
            source: RolePolicy {
                severity: Some(Severity::Error),
                clone_enabled: Some(true),
                clone_min_lines: Some(if strict { 5 } else { 8 }),
                clone_min_tokens: Some(if strict { 50 } else { 80 }),
                mutation_target: Some(true),
                ..Default::default()
            },
            test: RolePolicy {
                severity: Some(Severity::Error),
                clone_enabled: Some(true),
                clone_min_lines: Some(test_lines),
                clone_min_tokens: Some(test_tokens),
                mutation_target: Some(false),
                ..Default::default()
            },
            generated: RolePolicy {
                severity: Some(Severity::Ignore),
                clone_enabled: Some(false),
                mutation_target: Some(false),
                ..Default::default()
            },
            fixture: RolePolicy {
                severity: Some(Severity::Warning),
                clone_enabled: Some(true),
                clone_min_lines: Some(fixture_lines),
                clone_min_tokens: Some(fixture_tokens),
                mutation_target: Some(false),
                ..Default::default()
            },
            migration: RolePolicy {
                severity: Some(Severity::Error),
                clone_enabled: Some(false),
                mutation_target: Some(false),
                ..Default::default()
            },
        }
    }

    /// Overlay explicitly configured role sections while preserving preset
    /// values for omitted fields.
    pub fn merge_from(&mut self, overrides: &Self) {
        self.source.merge_from(&overrides.source);
        self.test.merge_from(&overrides.test);
        self.generated.merge_from(&overrides.generated);
        self.fixture.merge_from(&overrides.fixture);
        self.migration.merge_from(&overrides.migration);
    }

    /// Validate all role-specific thresholds and engine applicability rules.
    pub fn validate(&self) -> Result<()> {
        self.source.validate(FileRole::Source)?;
        self.test.validate(FileRole::Test)?;
        self.generated.validate(FileRole::Generated)?;
        self.fixture.validate(FileRole::Fixture)?;
        self.migration.validate(FileRole::Migration)?;
        Ok(())
    }

    /// Return the policy associated with a role, if that role has a first-
    /// class section.  Vendor/config/documentation/unknown roles inherit only
    /// global engine behavior and therefore return `None`.
    pub fn for_role(&self, role: FileRole) -> Option<&RolePolicy> {
        match role {
            FileRole::Source => Some(&self.source),
            FileRole::Test => Some(&self.test),
            FileRole::Generated => Some(&self.generated),
            FileRole::Fixture => Some(&self.fixture),
            FileRole::Migration => Some(&self.migration),
            FileRole::Vendor | FileRole::Config | FileRole::Documentation | FileRole::Unknown => {
                None
            }
        }
    }
}

/// Generated-file freshness policy.  This is intentionally separate from
/// budget exclusions: excluding a file from size checks must never disable a
/// generated artifact's freshness command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedConfig {
    #[serde(default)]
    pub enabled: bool,
    pub freshness_command: Option<String>,
    #[serde(default = "default_generated_timeout")]
    pub timeout_secs: Option<u64>,
}

impl Default for GeneratedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            freshness_command: None,
            timeout_secs: default_generated_timeout(),
        }
    }
}

impl GeneratedConfig {
    pub fn validate(&self) -> Result<()> {
        if self.enabled
            && self
                .freshness_command
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            bail!("generated.freshness_command is required when generated freshness is enabled");
        }
        if self
            .freshness_command
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("generated.freshness_command must not be empty");
        }
        if let Some(value) = self.timeout_secs {
            ensure_positive(value, "generated.timeout_secs")?;
        }
        Ok(())
    }
}

const fn default_generated_timeout() -> Option<u64> {
    Some(300)
}

/// Existing-code adoption settings carried by the legacy preset.
///
/// The ratchet engine is intentionally outside this configuration foundation;
/// these fields preserve the contract for the worker that implements it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LegacyConfig {
    pub reference_branch: Option<String>,
    #[serde(default)]
    pub ratchet: bool,
}

impl LegacyConfig {
    pub fn for_preset(legacy: bool) -> Self {
        Self {
            reference_branch: legacy.then(|| "origin/main".to_string()),
            ratchet: legacy,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.ratchet
            && self
                .reference_branch
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            bail!("legacy.reference_branch is required when legacy.ratchet is enabled");
        }
        if self
            .reference_branch
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("legacy.reference_branch must not be empty");
        }
        Ok(())
    }
}

/// Ordered, user-defined classification override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassificationRule {
    pub glob: String,
    pub role: FileRole,
}

/// Classification rules are evaluated in declaration order before built-ins,
/// except for vendor/build pruning which always remains authoritative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClassificationConfig {
    #[serde(default)]
    pub rules: Vec<ClassificationRule>,
}

impl ClassificationConfig {
    pub fn validate(&self) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        for (index, rule) in self.rules.iter().enumerate() {
            let normalized = rule.glob.trim().replace('\\', "/").to_ascii_lowercase();
            if normalized.is_empty() {
                bail!("classification.rules[{index}].glob must not be empty");
            }
            Glob::new(&normalized).map_err(|error| {
                anyhow::anyhow!("Invalid classification glob `{}`: {error}", rule.glob)
            })?;
            if !seen.insert(normalized) {
                bail!(
                    "classification.rules[{index}].glob duplicates an earlier rule; duplicate globs are ambiguous"
                );
            }
        }
        Ok(())
    }
}

fn role_name(role: FileRole) -> &'static str {
    [
        "source",
        "test",
        "generated",
        "fixture",
        "vendor",
        "migration",
        "config",
        "documentation",
        "unknown",
    ][role as usize]
}

pub(super) fn ensure_positive<T>(value: T, field: &str) -> Result<()>
where
    T: PartialEq + From<u8>,
{
    if value == T::from(0) {
        bail!("{field} must be greater than zero");
    }
    Ok(())
}

pub(super) fn ensure_positive_float(value: f64, field: &str) -> Result<()> {
    if !value.is_finite() || value <= 0.0 {
        bail!("{field} must be finite and greater than zero");
    }
    Ok(())
}

fn merge_option<T: Copy>(target: &mut Option<T>, override_value: &Option<T>) {
    if override_value.is_some() {
        *target = *override_value;
    }
}
