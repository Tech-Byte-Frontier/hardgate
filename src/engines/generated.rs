use crate::config::{GeneratedConfig, OrchestrationConfig};
use crate::engines::orchestration::{
    OrchestrationEngine, OrchestrationResult, OrchestrationStep, OrchestrationViolation,
};
use std::path::Path;

const GENERATED_FRESHNESS_STEP: &str = "generated-freshness";
const GENERATED_FRESHNESS_RECOMMENDATION: &str =
    "Regenerate generated artifacts and ensure generated.freshness_command exits successfully.";

/// Run the configured generated-artifact freshness check when enabled.
///
/// The freshness command gets its own orchestration engine so its timeout is
/// independent of formatter, linter, and test commands. Process execution and
/// output handling remain centralized in [`OrchestrationEngine::run_step`].
pub fn run_generated_freshness(
    config: &GeneratedConfig,
    root: &Path,
) -> Option<Result<OrchestrationResult, OrchestrationViolation>> {
    if !config.enabled {
        return None;
    }

    let Some(command) = config
        .freshness_command
        .as_deref()
        .filter(|command| !command.trim().is_empty())
    else {
        return Some(Err(missing_command_violation()));
    };

    let orchestration = OrchestrationConfig {
        timeout_secs: config.timeout_secs,
        ..OrchestrationConfig::default()
    };
    let engine = OrchestrationEngine::new(&orchestration);
    Some(engine.run_step(
        OrchestrationStep {
            step: GENERATED_FRESHNESS_STEP,
            command,
            recommendation: GENERATED_FRESHNESS_RECOMMENDATION,
        },
        root,
    ))
}

fn missing_command_violation() -> OrchestrationViolation {
    OrchestrationViolation {
        step: GENERATED_FRESHNESS_STEP.to_string(),
        command: String::new(),
        exit_code: Some(1),
        output: "Generated freshness is enabled but no freshness_command was configured."
            .to_string(),
        recommendation:
            "Set generated.freshness_command to a non-empty command that verifies generated artifacts."
                .to_string(),
    }
}
