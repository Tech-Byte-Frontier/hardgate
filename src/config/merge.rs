use super::{
    AntiGamingConfig, ClassificationConfig, CloneConfig, CoverageConfig, DeadCodeConfig,
    FileBudgets, FunctionBudgets, GateConfig, GeneratedConfig, HardgateConfig, InvariantsConfig,
    LegacyConfig, MutationConfig, OrchestrationConfig, RolePoliciesConfig,
};

macro_rules! set {
    ($table:expr, $field:ident, $base:expr, $user:expr) => {
        if $table.contains_key(stringify!($field)) {
            $base.$field = $user.$field.clone();
        }
    };
}

pub(super) fn merge_overrides(base: &mut HardgateConfig, user: HardgateConfig, raw: &toml::Table) {
    merge_static_overrides(base, &user, raw);
    merge_dynamic_overrides(base, &user, raw);
    merge_role_overrides(base, &user, raw);
    merge_gate(&mut base.gate, &user.gate, raw);
    merge_file_budgets(&mut base.budgets.files, user.budgets.files, raw);
    merge_func_budgets(&mut base.budgets.functions, user.budgets.functions, raw);
}

fn lookup_table<'a>(root: &'a toml::Table, path: &[&str]) -> Option<&'a toml::Table> {
    let mut current = root;
    for key in path {
        current = current.get(*key)?.as_table()?;
    }
    Some(current)
}

fn has_section(root: &toml::Table, path: &[&str]) -> bool {
    lookup_table(root, path).is_some()
}

fn with_section<F>(raw: &toml::Table, path: &[&str], apply: F)
where
    F: FnOnce(&toml::Table),
{
    if let Some(table) = lookup_table(raw, path) {
        apply(table);
    }
}

fn merge_static_overrides(base: &mut HardgateConfig, user: &HardgateConfig, raw: &toml::Table) {
    merge_anti_gaming(&mut base.anti_gaming, &user.anti_gaming, raw);
    merge_invariants(&mut base.invariants, &user.invariants, raw);
    merge_clones(&mut base.clones, &user.clones, raw);
}

fn merge_gate(base: &mut GateConfig, user: &GateConfig, raw: &toml::Table) {
    with_section(raw, &["gate"], |table| {
        set!(table, name, base, user);
        set!(table, preset, base, user);
        set!(table, strict, base, user);
        set!(table, enforce_classified_sources, base, user);
    });
}

fn merge_anti_gaming(base: &mut AntiGamingConfig, user: &AntiGamingConfig, raw: &toml::Table) {
    with_section(raw, &["anti_gaming"], |table| {
        set!(table, disallow_suppressions, base, user);
        set!(table, custom_forbidden_tokens, base, user);
    });
}

fn merge_invariants(base: &mut InvariantsConfig, user: &InvariantsConfig, raw: &toml::Table) {
    with_section(raw, &["invariants"], |table| {
        set!(table, enforce, base, user);
        set!(table, rules, base, user);
    });
}

fn merge_clones(base: &mut CloneConfig, user: &CloneConfig, raw: &toml::Table) {
    with_section(raw, &["clones"], |table| {
        set!(table, min_lines, base, user);
        set!(table, min_tokens, base, user);
        set!(table, excludes, base, user);
        set!(table, enabled, base, user);
    });
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
    merge_coverage(&mut base.coverage, &user.coverage, raw);
    merge_mutation(&mut base.mutation, &user.mutation, raw);
}

fn merge_coverage(base: &mut CoverageConfig, user: &CoverageConfig, raw: &toml::Table) {
    with_section(raw, &["coverage"], |table| {
        set!(table, report, base, user);
        set!(table, min_line_percent, base, user);
        set!(table, min_function_percent, base, user);
        set!(table, min_branch_percent, base, user);
        set!(table, max_crap_score, base, user);
        set!(table, critical_paths, base, user);
        set!(table, enabled, base, user);
    });
}

fn merge_mutation(base: &mut MutationConfig, user: &MutationConfig, raw: &toml::Table) {
    with_section(raw, &["mutation"], |table| {
        set!(table, min_score, base, user);
        set!(table, reject_timeouts, base, user);
        set!(table, reports, base, user);
        set!(table, test_cmd, base, user);
        set!(table, timeout_secs, base, user);
        set!(table, max_mutants, base, user);
        set!(table, enabled, base, user);
    });
}

fn merge_tooling_overrides(base: &mut HardgateConfig, user: &HardgateConfig, raw: &toml::Table) {
    merge_orchestration(&mut base.orchestration, &user.orchestration, raw);
    merge_dead_code(&mut base.analysis.dead_code, &user.analysis.dead_code, raw);
}

fn merge_dead_code(base: &mut DeadCodeConfig, user: &DeadCodeConfig, raw: &toml::Table) {
    with_section(raw, &["analysis", "dead_code"], |table| {
        set!(table, entry_points, base, user);
        set!(table, exclude, base, user);
        set!(table, enabled, base, user);
    });
}

fn merge_orchestration(
    base: &mut OrchestrationConfig,
    user: &OrchestrationConfig,
    raw: &toml::Table,
) {
    with_section(raw, &["orchestration"], |table| {
        set!(table, format_check, base, user);
        set!(table, format, base, user);
        set!(table, lint, base, user);
        set!(table, test_cmd, base, user);
        set!(table, timeout_secs, base, user);
    });
}

fn merge_role_overrides(base: &mut HardgateConfig, user: &HardgateConfig, raw: &toml::Table) {
    merge_role_policies(&mut base.roles, &user.roles, raw);
    merge_classification(&mut base.classification, &user.classification, raw);
    merge_generated(&mut base.generated, &user.generated, raw);
    merge_legacy(&mut base.legacy, &user.legacy, raw);
}

fn merge_role_policies(
    base: &mut RolePoliciesConfig,
    user: &RolePoliciesConfig,
    raw: &toml::Table,
) {
    if has_section(raw, &["roles"]) || has_section(raw, &["role_policies"]) {
        base.merge_from(user);
    }
}

fn merge_classification(
    base: &mut ClassificationConfig,
    user: &ClassificationConfig,
    raw: &toml::Table,
) {
    with_section(raw, &["classification"], |table| {
        set!(table, rules, base, user);
    });
}

fn merge_generated(base: &mut GeneratedConfig, user: &GeneratedConfig, raw: &toml::Table) {
    with_section(raw, &["generated"], |table| {
        set!(table, freshness_command, base, user);
        set!(table, timeout_secs, base, user);
        set!(table, enabled, base, user);
    });
}

fn merge_legacy(base: &mut LegacyConfig, user: &LegacyConfig, raw: &toml::Table) {
    with_section(raw, &["legacy"], |table| {
        set!(table, reference_branch, base, user);
        set!(table, ratchet, base, user);
    });
}

fn merge_file_budgets(base: &mut FileBudgets, user: FileBudgets, raw: &toml::Table) {
    let Some(table) = lookup_table(raw, &["budgets", "files"]) else {
        return;
    };
    set!(table, max_bytes, base, user);
    if let Some(lines) = table.get("max_lines").and_then(toml::Value::as_table) {
        if lines.is_empty() {
            base.max_lines.clear();
        }
        for key in lines.keys() {
            if let Some(value) = user.max_lines.get(key) {
                base.max_lines.insert(key.clone(), *value);
            }
        }
    }
    if let Some(exclusions) = table.get("exclusions").and_then(toml::Value::as_table) {
        if exclusions.contains_key("paths") {
            base.exclusions.paths = user.exclusions.paths;
        }
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
