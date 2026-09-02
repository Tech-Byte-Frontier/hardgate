use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    AntiGamingConfig, BudgetsConfig, CloneConfig, CoverageConfig, FileBudgets, FunctionBudgets,
    GateConfig, HardgateConfig, InvariantsConfig, MutationConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    #[default]
    StrictAgent,
    Balanced,
    LegacyMigration,
    Custom,
}

impl Preset {
    pub fn apply_to(&self, config: &mut HardgateConfig) {
        if *self == Preset::Custom {
            return;
        }

        let b = get_preset_bundle(*self == Preset::StrictAgent);

        config.budgets.files = FileBudgets {
            max_bytes: Some(b.max_bytes),
            max_lines: build_line_budgets(b.lines.0, b.lines.1, b.lines.2),
            exclusions: Default::default(),
        };
        config.budgets.functions = make_func_budgets(b.thresholds);
        config.anti_gaming.disallow_suppressions = true;
        config.clones = make_clones(b.clones.0, b.clones.1);
        config.coverage = make_coverage(b.coverage.0, b.coverage.1, b.coverage.2);
        config.mutation = make_mutation(b.mutation.0, b.mutation.1, b.mutation.2);

        if *self == Preset::LegacyMigration {
            config.gate.strict = false;
        }
    }

    pub fn to_default_config(self) -> HardgateConfig {
        let mut config = HardgateConfig {
            gate: GateConfig {
                name: "hardgate-project".to_string(),
                preset: self,
                strict: true,
                enforce_classified_sources: false,
            },
            budgets: BudgetsConfig::default(),
            anti_gaming: AntiGamingConfig::default(),
            invariants: InvariantsConfig::default(),
            clones: CloneConfig::default(),
            coverage: CoverageConfig::default(),
            mutation: MutationConfig::default(),
            orchestration: Default::default(),
            analysis: Default::default(),
        };
        self.apply_to(&mut config);
        config
    }

    pub fn to_clean_toml(self) -> String {
        let (preset_str, b) = self.preset_toml_context();
        format_toml_template(preset_str, &b)
    }

    fn preset_toml_context(self) -> (&'static str, PresetBudgetContext) {
        let preset_str = match self {
            Preset::StrictAgent => "strict-agent",
            Preset::Balanced => "balanced",
            Preset::LegacyMigration => "legacy-migration",
            Preset::Custom => "custom",
        };

        let b = if self == Preset::StrictAgent {
            PresetBudgetContext {
                max_bytes: 32768,
                cyclo: 10,
                cogn: 15,
                halstead: 80.0,
                params: 4,
                lines: 80,
                depth: 4,
                clones_l: 5,
                clones_t: 50,
                cov_min: 95.0,
                mut_min: 85.0,
            }
        } else {
            PresetBudgetContext {
                max_bytes: 65536,
                cyclo: 15,
                cogn: 22,
                halstead: 120.0,
                params: 6,
                lines: 120,
                depth: 6,
                clones_l: 8,
                clones_t: 80,
                cov_min: 80.0,
                mut_min: 75.0,
            }
        };

        (preset_str, b)
    }
}

struct PresetBudgetContext {
    max_bytes: usize,
    cyclo: usize,
    cogn: usize,
    halstead: f64,
    params: usize,
    lines: usize,
    depth: usize,
    clones_l: usize,
    clones_t: usize,
    cov_min: f64,
    mut_min: f64,
}

fn format_toml_template(preset_str: &str, b: &PresetBudgetContext) -> String {
    format!(
        r#"# Hardgate deterministic quality gate configuration
# Documentation: https://github.com/Tech-Byte-Frontier/hardgate

[gate]
name = "project"
preset = "{preset_str}"
strict = true

[budgets.files]
max_bytes = {max_bytes}

[budgets.files.exclusions]
paths = [
  "tests/**",
]

[budgets.files.max_lines]
rs = 499
ts = 400
tsx = 400
js = 400
default = 350

[budgets.functions]
max_cyclomatic = {cyclo}
max_cognitive = {cogn}
max_halstead_difficulty = {halstead:.1}
max_parameters = {params}
max_lines = {lines}
max_nesting_depth = {depth}

[anti_gaming]
disallow_suppressions = true

[clones]
min_lines = {clones_l}
min_tokens = {clones_t}

[coverage]
enabled = false
report = "coverage/lcov.info"
min_line_percent = {cov_min:.1}
max_crap_score = 25.0

[mutation]
enabled = false
min_score = {mut_min:.1}
timeout_secs = 10
max_mutants = 30

[orchestration]
format_check = "oxfmt --check ."
format = "oxfmt ."
lint = "oxlint --type-aware ."

[analysis.dead_code]
enabled = false
entry_points = [
  "src/main.rs",
  "src/lib.rs",
  "src/index.ts",
  "src/index.tsx",
]
"#,
        max_bytes = b.max_bytes,
        cyclo = b.cyclo,
        cogn = b.cogn,
        halstead = b.halstead,
        params = b.params,
        lines = b.lines,
        depth = b.depth,
        clones_l = b.clones_l,
        clones_t = b.clones_t,
        cov_min = b.cov_min,
        mut_min = b.mut_min,
    )
}

fn build_line_budgets(rs: usize, other: usize, default_val: usize) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    map.insert("rs".to_string(), rs);
    for ext in &["ts", "tsx", "js", "jsx", "py", "go"] {
        map.insert(ext.to_string(), other);
    }
    map.insert("default".to_string(), default_val);
    map
}

fn make_clones(min_lines: usize, min_tokens: usize) -> CloneConfig {
    CloneConfig {
        enabled: true,
        min_lines,
        min_tokens,
        excludes: None,
    }
}

fn make_coverage(enabled: bool, line_pct: f64, max_crap: f64) -> CoverageConfig {
    CoverageConfig {
        enabled,
        report: if enabled {
            Some("coverage/lcov.info".to_string())
        } else {
            None
        },
        min_line_percent: Some(line_pct),
        min_function_percent: Some(line_pct),
        min_branch_percent: Some(line_pct - 5.0),
        max_crap_score: Some(max_crap),
        critical_paths: None,
    }
}

fn make_mutation(enabled: bool, score: f64, reject_timeouts: bool) -> MutationConfig {
    MutationConfig {
        enabled,
        min_score: Some(score),
        reject_timeouts,
        reports: None,
        test_cmd: None,
        timeout_secs: Some(10),
        max_mutants: Some(30),
    }
}

struct FuncThresholds {
    cyclo: u32,
    cogn: u32,
    halstead: f64,
    params: usize,
    lines: usize,
    depth: usize,
}

fn make_func_budgets(t: FuncThresholds) -> FunctionBudgets {
    FunctionBudgets {
        max_cyclomatic: Some(t.cyclo),
        max_cognitive: Some(t.cogn),
        max_halstead_difficulty: Some(t.halstead),
        max_abc: Some(100.0),
        max_parameters: Some(t.params),
        max_lines: Some(t.lines),
        max_statements: Some(30),
        max_nesting_depth: Some(t.depth),
    }
}

struct PresetBundle {
    max_bytes: u64,
    lines: (usize, usize, usize),
    thresholds: FuncThresholds,
    clones: (usize, usize),
    coverage: (bool, f64, f64),
    mutation: (bool, f64, bool),
}

fn get_preset_bundle(strict: bool) -> PresetBundle {
    let scale = if strict { 1.0 } else { 1.5 };
    PresetBundle {
        max_bytes: if strict { 32768 } else { 65536 },
        lines: if strict {
            (499, 400, 350)
        } else {
            (600, 500, 500)
        },
        thresholds: FuncThresholds {
            cyclo: (10.0 * scale) as u32,
            cogn: (15.0 * scale) as u32,
            halstead: 80.0 * scale,
            params: if strict { 4 } else { 6 },
            lines: if strict { 80 } else { 120 },
            depth: if strict { 4 } else { 6 },
        },
        clones: if strict { (5, 50) } else { (8, 80) },
        coverage: if strict {
            (true, 95.0, 25.0)
        } else {
            (false, 80.0, 30.0)
        },
        mutation: if strict {
            (true, 85.0, true)
        } else {
            (false, 75.0, false)
        },
    }
}
