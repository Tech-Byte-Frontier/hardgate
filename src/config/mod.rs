mod merge;
pub mod preset;
mod roles;
mod validation;

use anyhow::{Context, Result};
pub use preset::Preset;
pub use roles::{
    ClassificationConfig, ClassificationRule, GeneratedConfig, LegacyConfig, RolePoliciesConfig,
    RolePolicy, Severity,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Root `hardgate.toml` configuration: gate identity plus every engine budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardgateConfig {
    #[serde(default)]
    pub gate: GateConfig,
    #[serde(default)]
    pub budgets: BudgetsConfig,
    #[serde(default)]
    pub anti_gaming: AntiGamingConfig,
    #[serde(default)]
    pub invariants: InvariantsConfig,
    #[serde(default)]
    pub clones: CloneConfig,
    #[serde(default)]
    pub coverage: CoverageConfig,
    #[serde(default)]
    pub mutation: MutationConfig,
    #[serde(default)]
    pub orchestration: OrchestrationConfig,
    #[serde(default)]
    pub analysis: AnalysisConfig,
    /// Role-specific policy overrides.  Fields omitted here inherit global
    /// engine budgets for backwards-compatible TOML.
    #[serde(default, alias = "role_policies")]
    pub roles: RolePoliciesConfig,
    /// Ordered custom classification rules evaluated before built-ins.
    #[serde(default)]
    pub classification: ClassificationConfig,
    /// Generated artifact freshness command, independent from exclusions.
    #[serde(default)]
    pub generated: GeneratedConfig,
    /// Existing-code adoption reference branch and ratchet contract.
    #[serde(default)]
    pub legacy: LegacyConfig,
}

impl Default for HardgateConfig {
    fn default() -> Self {
        Preset::StrictAgent.to_default_config()
    }
}

/// Gate identity: display name, base preset, and strictness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateConfig {
    #[serde(default = "default_gate_name")]
    pub name: String,
    #[serde(default)]
    pub preset: Preset,
    #[serde(default = "default_true")]
    pub strict: bool,
    #[serde(default)]
    pub enforce_classified_sources: bool,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            name: default_gate_name(),
            preset: Preset::StrictAgent,
            strict: true,
            enforce_classified_sources: false,
        }
    }
}

fn default_gate_name() -> String {
    "project".to_string()
}

fn default_true() -> bool {
    true
}

/// Physical file budgets plus per-function AST complexity budgets.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetsConfig {
    #[serde(default)]
    pub files: FileBudgets,
    #[serde(default)]
    pub functions: FunctionBudgets,
}

/// Byte/line ceilings per file, with glob exclusions that surface advisories.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileBudgets {
    pub max_bytes: Option<u64>,
    #[serde(default)]
    pub max_lines: HashMap<String, usize>,
    #[serde(default)]
    pub exclusions: ExclusionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExclusionConfig {
    #[serde(default)]
    pub paths: Vec<String>,
}

/// Per-function ceilings: cyclomatic, cognitive, Halstead, ABC, parameters,
/// lines, statements, and nesting depth.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FunctionBudgets {
    pub max_cyclomatic: Option<u32>,
    pub max_cognitive: Option<u32>,
    pub max_halstead_difficulty: Option<f64>,
    pub max_abc: Option<f64>,
    pub max_parameters: Option<usize>,
    pub max_lines: Option<usize>,
    pub max_statements: Option<usize>,
    pub max_nesting_depth: Option<usize>,
}

/// Zero-tolerance suppression policy plus project-specific forbidden tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiGamingConfig {
    #[serde(default = "default_true")]
    pub disallow_suppressions: bool,
    #[serde(default)]
    pub custom_forbidden_tokens: Vec<String>,
}

impl Default for AntiGamingConfig {
    fn default() -> Self {
        Self {
            disallow_suppressions: true,
            custom_forbidden_tokens: Vec::new(),
        }
    }
}

/// Architectural boundary rules between subsystems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantsConfig {
    #[serde(default = "default_true")]
    pub enforce: bool,
    #[serde(default)]
    pub rules: Vec<InvariantRule>,
}

impl Default for InvariantsConfig {
    fn default() -> Self {
        Self {
            enforce: true,
            rules: Vec::new(),
        }
    }
}

/// One boundary rule: which files it covers and what imports, calls, or
/// tokens are forbidden there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantRule {
    pub name: Option<String>,
    pub from: String,
    pub exclude: Option<Vec<String>>,
    pub disallow_imports: Option<Vec<String>>,
    pub disallow_calls: Option<Vec<String>>,
    pub disallow_tokens: Option<Vec<String>>,
    pub message: Option<String>,
}

/// Token-stream clone detection thresholds and exclusion globs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_min_clone_lines")]
    pub min_lines: usize,
    #[serde(default = "default_min_clone_tokens")]
    pub min_tokens: usize,
    pub excludes: Option<Vec<String>>,
}

impl Default for CloneConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_lines: default_min_clone_lines(),
            min_tokens: default_min_clone_tokens(),
            excludes: None,
        }
    }
}

fn default_min_clone_lines() -> usize {
    5
}

fn default_min_clone_tokens() -> usize {
    50
}

/// Coverage floors (line/function/branch), CRAP ceiling, and critical paths
/// requiring full coverage.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverageConfig {
    #[serde(default)]
    pub enabled: bool,
    pub report: Option<String>,
    pub min_line_percent: Option<f64>,
    pub min_function_percent: Option<f64>,
    pub min_branch_percent: Option<f64>,
    pub max_crap_score: Option<f64>,
    pub critical_paths: Option<Vec<String>>,
}

/// Mutation testing policy: kill-rate floor, timeout handling, and runner tuning.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MutationConfig {
    #[serde(default)]
    pub enabled: bool,
    pub min_score: Option<f64>,
    #[serde(default = "default_true")]
    pub reject_timeouts: bool,
    pub reports: Option<Vec<String>>,
    pub test_cmd: Option<String>,
    pub timeout_secs: Option<u64>,
    pub max_mutants: Option<usize>,
}

/// External formatter/linter/test commands orchestrated by `fmt` and `check --all`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrchestrationConfig {
    pub format_check: Option<String>,
    pub format: Option<String>,
    pub lint: Option<String>,
    pub test_cmd: Option<String>,
    /// Maximum runtime for an orchestrated command, in seconds.
    pub timeout_secs: Option<u64>,
}

/// Post-static analyses such as dead-code detection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisConfig {
    #[serde(default)]
    pub dead_code: DeadCodeConfig,
}

/// Dead-code detection: entry points plus exclusion globs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeadCodeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub entry_points: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl HardgateConfig {
    /// Load `hardgate.toml` (or `path`), falling back to the strict-agent
    /// preset when no config file exists.
    pub fn load_or_default(path: Option<&Path>) -> Result<Self> {
        let config_path = match path {
            Some(p) => p.to_path_buf(),
            None => PathBuf::from("hardgate.toml"),
        };

        if !config_path.exists() {
            let config = Preset::StrictAgent.to_default_config();
            config.validate()?;
            return Ok(config);
        }

        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file at {:?}", config_path))?;

        let mut config: HardgateConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file at {:?}", config_path))?;

        // Apply preset base defaults if preset is not custom.
        // Merge is presence-based: only sections/keys present in the user's
        // TOML override the preset base, so explicit `enabled = false` is
        // honored while omitted sections keep preset scaling (e.g. balanced).
        if config.gate.preset != Preset::Custom {
            let raw_table: toml::Table = toml::from_str(&content).unwrap_or_default();
            let mut base = config.gate.preset.to_default_config();
            merge::merge_overrides(&mut base, config, &raw_table);
            config = base;
        }

        config.validate()?;
        Ok(config)
    }

    /// Render a commented `hardgate.toml` template for `preset` (`init` output).
    pub fn generate_toml_template(preset: Preset) -> String {
        preset.to_clean_toml()
    }

    /// Validate all user-controlled configuration before an engine executes.
    ///
    /// Serde catches malformed enum values and TOML types; this pass catches
    /// semantically unsafe values such as zero thresholds, invalid globs, and
    /// enabled freshness without a command.
    pub fn validate(&self) -> Result<()> {
        validation::validate(self)
    }
}
