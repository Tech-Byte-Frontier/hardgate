use super::CommandResult;
use crate::diagnostics::execution::{EngineState, ExecutionPlan};

/// Retains validated policy and intended scope when an evaluation aborts.
#[derive(Debug)]
pub struct ExecutionFailure {
    pub plan: ExecutionPlan,
}

impl std::fmt::Display for ExecutionFailure {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{} evaluation did not finish", self.plan.command)
    }
}

pub(crate) fn run_planned(
    plan: ExecutionPlan,
    run: impl FnOnce(ExecutionPlan) -> CommandResult,
) -> CommandResult {
    let mut failed = plan.clone();
    run(plan).map_err(|error| {
        for engine in &mut failed.engines {
            if engine.selected {
                engine.state = EngineState::Incomplete;
                engine.reason =
                    Some("command aborted; complete engine evidence was not returned".into());
            }
        }
        error.context(ExecutionFailure { plan: failed })
    })
}
