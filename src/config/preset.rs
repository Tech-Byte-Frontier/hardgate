use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    AntiGamingConfig, BudgetsConfig, CloneConfig, CoverageConfig, FileBudgets,
    FunctionBudgets, GateConfig, HardgateConfig, InvariantsConfig, MutationConfig,
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
        };
        self.apply_to(&mut config);
        config
    }
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
        report: if enabled { Some("coverage/lcov.info".to_string()) } else { None },
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
        lines: if strict { (499, 400, 350) } else { (600, 500, 500) },
        thresholds: FuncThresholds {
            cyclo: (10.0 * scale) as u32,
            cogn: (15.0 * scale) as u32,
            halstead: 80.0 * scale,
            params: if strict { 4 } else { 6 },
            lines: if strict { 80 } else { 120 },
            depth: if strict { 4 } else { 6 },
        },
        clones: if strict { (5, 50) } else { (8, 80) },
        coverage: if strict { (true, 95.0, 25.0) } else { (false, 80.0, 30.0) },
        mutation: if strict { (true, 85.0, true) } else { (false, 75.0, false) },
    }
}
