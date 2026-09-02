pub mod preset;

use anyhow::{Context, Result};
pub use preset::Preset;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetsConfig {
    #[serde(default)]
    pub files: FileBudgets,
    #[serde(default)]
    pub functions: FunctionBudgets,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvariantsConfig {
    #[serde(default = "default_true")]
    pub enforce: bool,
    #[serde(default)]
    pub rules: Vec<InvariantRule>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrchestrationConfig {
    pub format_check: Option<String>,
    pub format: Option<String>,
    pub lint: Option<String>,
    pub test_cmd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisConfig {
    #[serde(default)]
    pub dead_code: DeadCodeConfig,
}

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
    pub fn load_or_default(path: Option<&Path>) -> Result<Self> {
        let config_path = match path {
            Some(p) => p.to_path_buf(),
            None => PathBuf::from("hardgate.toml"),
        };

        if !config_path.exists() {
            return Ok(Preset::StrictAgent.to_default_config());
        }

        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file at {:?}", config_path))?;

        let mut config: HardgateConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file at {:?}", config_path))?;

        // Apply preset base defaults if preset is not custom
        if config.gate.preset != Preset::Custom {
            let mut base = config.gate.preset.to_default_config();
            merge_overrides(&mut base, config);
            config = base;
        }

        Ok(config)
    }

    pub fn generate_toml_template(preset: Preset) -> String {
        preset.to_clean_toml()
    }
}

fn merge_overrides(base: &mut HardgateConfig, user: HardgateConfig) {
    merge_static_overrides(base, &user);
    merge_dynamic_overrides(base, &user);
    base.gate = user.gate;
    merge_file_budgets(&mut base.budgets.files, user.budgets.files);
    merge_func_budgets(&mut base.budgets.functions, user.budgets.functions);
}

fn merge_static_overrides(base: &mut HardgateConfig, user: &HardgateConfig) {
    if !user.anti_gaming.custom_forbidden_tokens.is_empty() {
        base.anti_gaming.custom_forbidden_tokens = user.anti_gaming.custom_forbidden_tokens.clone();
    }
    if !user.invariants.rules.is_empty() {
        base.invariants.rules = user.invariants.rules.clone();
    }
    if user.clones.excludes.is_some() {
        base.clones = user.clones.clone();
    }
}

fn merge_dynamic_overrides(base: &mut HardgateConfig, user: &HardgateConfig) {
    merge_verification_overrides(base, user);
    merge_tooling_overrides(base, user);
}

fn merge_verification_overrides(base: &mut HardgateConfig, user: &HardgateConfig) {
    if user.coverage.report.is_some() || user.coverage.enabled {
        base.coverage = user.coverage.clone();
    }
    if user.mutation.reports.is_some() || user.mutation.test_cmd.is_some() || user.mutation.enabled
    {
        base.mutation = user.mutation.clone();
    }
}

fn merge_tooling_overrides(base: &mut HardgateConfig, user: &HardgateConfig) {
    if user.orchestration.format_check.is_some()
        || user.orchestration.format.is_some()
        || user.orchestration.lint.is_some()
    {
        base.orchestration = user.orchestration.clone();
    }
    if user.analysis.dead_code.enabled || !user.analysis.dead_code.entry_points.is_empty() {
        base.analysis = user.analysis.clone();
    }
}

fn merge_file_budgets(base: &mut FileBudgets, user: FileBudgets) {
    if user.max_bytes.is_some() {
        base.max_bytes = user.max_bytes;
    }
    for (k, v) in user.max_lines {
        base.max_lines.insert(k, v);
    }
    if !user.exclusions.paths.is_empty() {
        base.exclusions = user.exclusions;
    }
}

fn merge_func_budgets(base: &mut FunctionBudgets, user: FunctionBudgets) {
    if user.max_cyclomatic.is_some() {
        base.max_cyclomatic = user.max_cyclomatic;
    }
    if user.max_cognitive.is_some() {
        base.max_cognitive = user.max_cognitive;
    }
    if user.max_halstead_difficulty.is_some() {
        base.max_halstead_difficulty = user.max_halstead_difficulty;
    }
    if user.max_parameters.is_some() {
        base.max_parameters = user.max_parameters;
    }
    if user.max_lines.is_some() {
        base.max_lines = user.max_lines;
    }
    if user.max_nesting_depth.is_some() {
        base.max_nesting_depth = user.max_nesting_depth;
    }
}
