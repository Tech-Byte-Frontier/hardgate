use super::GateReport;
use crate::commands::CommandOutcome;
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct MachineOutcome<'a> {
    schema_version: u32,
    command: &'a str,
    status: &'static str,
    exit_code: u8,
    partial: bool,
    accepted: bool,
    omitted_requirements: Vec<super::execution::EngineId>,
}

impl<'a> MachineOutcome<'a> {
    pub(super) fn from_report(report: &'a GateReport) -> Self {
        let outcome = CommandOutcome::from_report(report);
        Self {
            schema_version: 1,
            command: report
                .execution
                .as_ref()
                .map_or("analysis", |plan| &plan.command),
            status: outcome.status(),
            exit_code: outcome.exit_code(),
            partial: report
                .execution
                .as_ref()
                .is_none_or(|plan| plan.is_partial()),
            accepted: report.passed
                && report.execution.as_ref().is_some_and(|plan| {
                    !plan.is_partial()
                        && plan.engines.iter().all(|engine| {
                            !engine.selected
                                || matches!(
                                    engine.state,
                                    super::execution::EngineState::Completed
                                        | super::execution::EngineState::Cached
                                )
                        })
                }),
            omitted_requirements: report
                .execution
                .as_ref()
                .map(|plan| plan.omitted_requirements())
                .unwrap_or_default(),
        }
    }
}
