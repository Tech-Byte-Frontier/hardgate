use crate::config::ConfigContext;
use crate::diagnostics::execution::{
    ConfigIdentity, EngineExecution, EngineId, EngineState, ExecutionPlan, ExecutionScope,
};
use crate::discovery::FileRole;
use std::path::PathBuf;

pub(crate) struct GateSelection<'a> {
    pub command: &'a str,
    pub paths: &'a [PathBuf],
    pub diff: bool,
    pub dead_code: bool,
    pub all: bool,
    pub coverage_report: Option<&'a str>,
    pub mutation_report: Option<&'a str>,
}

pub(crate) fn gate_plan(
    context: &ConfigContext,
    selection: GateSelection<'_>,
) -> anyhow::Result<ExecutionPlan> {
    let config = &context.config;
    let scan = matches!(selection.command, "scan" | "mcp_scan");
    let mut engines = Vec::new();
    for (id, enabled) in [
        (EngineId::FileBudgets, true),
        (EngineId::Complexity, true),
        (
            EngineId::Suppressions,
            config.anti_gaming.disallow_suppressions,
        ),
        (EngineId::Invariants, config.invariants.enforce),
        (EngineId::Clones, clones_enabled(config)),
        (
            EngineId::DeadCode,
            selection.dead_code || config.analysis.dead_code.enabled,
        ),
    ] {
        let static_requested = selection.command != "mutate";
        let mcp_dead_code = selection.command == "mcp_check" && id == EngineId::DeadCode;
        let selected = static_requested
            && !mcp_dead_code
            && (!scan || !matches!(id, EngineId::Clones | EngineId::DeadCode));
        engines.push(engine(
            id,
            enabled,
            selected,
            vec!["classified source snapshot".into()],
        ));
    }
    add_evidence_engines(&mut engines, context, &selection);
    add_commands(&mut engines, context, selection.all && !scan);
    Ok(ExecutionPlan {
        command: selection.command.to_string(),
        scope: ExecutionScope {
            mode: scope_mode(&selection).into(),
            paths: selection.paths.to_vec(),
        },
        config: ConfigIdentity::from_context(context)?,
        engines,
    })
}

fn scope_mode(selection: &GateSelection<'_>) -> &'static str {
    if selection.diff {
        "diff"
    } else if selection.paths.is_empty() {
        "repository"
    } else {
        "paths"
    }
}

fn clones_enabled(config: &crate::config::HardgateConfig) -> bool {
    FileRole::POLICY_ROLES.into_iter().any(|role| {
        let policy = config
            .roles
            .for_role(role)
            .and_then(|value| value.clone_enabled);
        (role.receives_clone_analysis() || policy == Some(true))
            && policy.unwrap_or(config.clones.enabled)
    })
}

fn add_evidence_engines(
    engines: &mut Vec<EngineExecution>,
    context: &ConfigContext,
    selection: &GateSelection<'_>,
) {
    let config = &context.config;
    let selected = matches!(selection.command, "check" | "verify");
    let coverage = selection
        .coverage_report
        .or(config.coverage.report.as_deref());
    engines.push(engine(
        EngineId::Coverage,
        config.coverage.enabled,
        selected,
        requirement(coverage, "coverage report path is not configured"),
    ));
    let mut mutation = selection
        .mutation_report
        .map(|value| vec![value.to_string()])
        .or_else(|| config.mutation.reports.clone())
        .unwrap_or_default();
    if mutation.is_empty() {
        mutation.push("mutation report paths are not configured".into());
    }
    engines.push(engine(
        EngineId::MutationReport,
        config.mutation.enabled,
        selected,
        mutation,
    ));
    engines.push(engine(
        EngineId::MutationExecution,
        config.mutation.enabled,
        selection.command == "mutate",
        vec!["a successful baseline and non-empty isolated mutation run (hardgate mutate)".into()],
    ));
    engines.push(engine(
        EngineId::GeneratedFreshness,
        config.generated.enabled,
        selected,
        requirement(
            config.generated.freshness_command.as_deref(),
            "generated.freshness_command is not configured",
        ),
    ));
    engines.push(engine(
        EngineId::LegacyRatchet,
        config.legacy.ratchet,
        selected,
        requirement(
            config.legacy.reference_branch.as_deref(),
            "legacy.reference_branch is not configured",
        ),
    ));
}

fn add_commands(engines: &mut Vec<EngineExecution>, context: &ConfigContext, selected: bool) {
    let config = &context.config.orchestration;
    for (id, command) in [
        (EngineId::FormatCheck, &config.format_check),
        (EngineId::Lint, &config.lint),
        (EngineId::Tests, &config.test_cmd),
    ] {
        engines.push(engine(
            id,
            command.is_some(),
            selected,
            command.iter().cloned().collect(),
        ));
    }
}

fn requirement(value: Option<&str>, missing: &str) -> Vec<String> {
    vec![value.unwrap_or(missing).to_string()]
}

fn engine(
    id: EngineId,
    enabled: bool,
    requested: bool,
    required_evidence: Vec<String>,
) -> EngineExecution {
    let selected = enabled && requested;
    let reason = if !enabled {
        "disabled by policy or unconfigured"
    } else if !selected {
        "not requested by this command"
    } else {
        "no eligible inputs evaluated"
    };
    EngineExecution {
        id,
        enabled,
        selected,
        required_evidence,
        state: if enabled {
            EngineState::Skipped
        } else {
            EngineState::Disabled
        },
        reason: Some(reason.into()),
    }
}
