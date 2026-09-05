use super::GateReport;
use crate::commands::CommandOutcome;
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct MachineOutcome<'a> {
    schema_version: u32,
    command: &'a str,
    status: &'static str,
    exit_code: u8,
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
        }
    }
}
