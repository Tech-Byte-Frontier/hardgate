use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::diagnostics::execution::{EngineId, EngineState};
use crate::discovery::ClassifiedFile;

pub(super) fn observe_file(
    file: &ClassifiedFile,
    config: &HardgateConfig,
    report: &mut GateReport,
) {
    for (id, ran) in [
        (EngineId::FileBudgets, file.role.receives_safety_checks()),
        (
            EngineId::Suppressions,
            file.role.receives_safety_checks() && config.anti_gaming.disallow_suppressions,
        ),
        (
            EngineId::Invariants,
            super::receives_invariants(file) && config.invariants.enforce,
        ),
        (
            EngineId::Complexity,
            file.role.receives_complexity() && file.ast_supported,
        ),
    ] {
        if ran {
            report.observe_engine(id, EngineState::Completed);
        }
    }
}
