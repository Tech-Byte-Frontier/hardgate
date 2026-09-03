pub mod preset;

use anyhow::{Context, Result};
pub use preset::Preset;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Root `hardgate.toml` configuration: gate identity plus every engine budget.
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvariantsConfig {
    #[serde(default = "default_true")]
    pub enforce: bool,
    #[serde(default)]
    pub rules: Vec<InvariantRule>,
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
            return Ok(Preset::StrictAgent.to_default_config());
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
            merge_overrides(&mut base, config, &raw_table);
            config = base;
        }

        Ok(config)
    }

    /// Render a commented `hardgate.toml` template for `preset` (`init` output).
    pub fn generate_toml_template(preset: Preset) -> String {
        preset.to_clean_toml()
    }
}

fn lookup_table<'a>(root: &'a toml::Table, path: &[&str]) -> Option<&'a toml::Table> {
    let mut cur = root;
    for key in path {
        let v = cur.get(*key)?;
        cur = v.as_table()?;
    }
    Some(cur)
}

fn has_section(root: &toml::Table, path: &[&str]) -> bool {
    lookup_table(root, path).is_some()
}

fn merge_overrides(base: &mut HardgateConfig, user: HardgateConfig, raw: &toml::Table) {
    merge_static_overrides(base, &user, raw);
    merge_dynamic_overrides(base, &user, raw);
    if has_section(raw, &["gate"]) {
        base.gate = user.gate;
    }
    merge_file_budgets(&mut base.budgets.files, user.budgets.files, raw);
    merge_func_budgets(&mut base.budgets.functions, user.budgets.functions, raw);
}

fn merge_static_overrides(base: &mut HardgateConfig, user: &HardgateConfig, raw: &toml::Table) {
    if has_section(raw, &["anti_gaming"]) {
        base.anti_gaming = user.anti_gaming.clone();
    } else if !user.anti_gaming.custom_forbidden_tokens.is_empty() {
        base.anti_gaming.custom_forbidden_tokens = user.anti_gaming.custom_forbidden_tokens.clone();
    }
    if has_section(raw, &["invariants"]) || !user.invariants.rules.is_empty() {
        // Presence wins; fallback preserves pre-presence partial files.
        if has_section(raw, &["invariants"]) {
            base.invariants = user.invariants.clone();
        } else {
            base.invariants.rules = user.invariants.rules.clone();
        }
    }
    if has_section(raw, &["clones"]) || user.clones.excludes.is_some() {
        base.clones = user.clones.clone();
    }
}

fn merge_dynamic_overrides(base: &mut HardgateConfig, user: &HardgateConfig, raw: &toml::Table) {
    merge_verification_overrides(base, user, raw);
    merge_tooling_overrides(base, user, raw);
}

fn merge_verification_overrides(
    base: &mut HardgateConfig,
    user: &HardgateConfig,
    raw: &toml::Table,
) {
    if has_section(raw, &["coverage"]) || user.coverage.report.is_some() || user.coverage.enabled {
        base.coverage = user.coverage.clone();
    }
    if has_section(raw, &["mutation"])
        || user.mutation.reports.is_some()
        || user.mutation.test_cmd.is_some()
        || user.mutation.enabled
    {
        base.mutation = user.mutation.clone();
    }
}

fn merge_tooling_overrides(base: &mut HardgateConfig, user: &HardgateConfig, raw: &toml::Table) {
    if has_section(raw, &["orchestration"])
        || user.orchestration.format_check.is_some()
        || user.orchestration.format.is_some()
        || user.orchestration.lint.is_some()
    {
        base.orchestration = user.orchestration.clone();
    }
    if has_section(raw, &["analysis", "dead_code"])
        || user.analysis.dead_code.enabled
        || !user.analysis.dead_code.entry_points.is_empty()
    {
        base.analysis = user.analysis.clone();
    }
}

fn merge_file_budgets(base: &mut FileBudgets, user: FileBudgets, _raw: &toml::Table) {
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

fn merge_func_budgets(base: &mut FunctionBudgets, user: FunctionBudgets, _raw: &toml::Table) {
    // All fields are `Option`: omitted keys deserialize to `None`, so
    // `is_some` alone distinguishes explicit user values from absent ones.
    // This also adds the previously missing `max_abc` / `max_statements`.
    if user.max_cyclomatic.is_some() {
        base.max_cyclomatic = user.max_cyclomatic;
    }
    if user.max_cognitive.is_some() {
        base.max_cognitive = user.max_cognitive;
    }
    if user.max_halstead_difficulty.is_some() {
        base.max_halstead_difficulty = user.max_halstead_difficulty;
    }
    if user.max_abc.is_some() {
        base.max_abc = user.max_abc;
    }
    if user.max_parameters.is_some() {
        base.max_parameters = user.max_parameters;
    }
    if user.max_lines.is_some() {
        base.max_lines = user.max_lines;
    }
    if user.max_statements.is_some() {
        base.max_statements = user.max_statements;
    }
    if user.max_nesting_depth.is_some() {
        base.max_nesting_depth = user.max_nesting_depth;
    }
}
