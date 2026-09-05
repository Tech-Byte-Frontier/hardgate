use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::diagnostics::execution::{EngineId, EngineState};
use crate::discovery::ClassifiedFile;

pub(super) fn observe_files<'a>(
    files: impl IntoIterator<Item = &'a ClassifiedFile>,
    config: &HardgateConfig,
    report: &mut GateReport,
) {
    let mut safety = false;
    let mut invariants = false;
    let mut complexity = false;
    for file in files {
        safety |= file.role.receives_safety_checks();
        invariants |= super::receives_invariants(file);
        complexity |= file.role.receives_complexity() && file.ast_supported;
        if safety && complexity && (invariants || !config.invariants.enforce) {
            break;
        }
    }
    // Completion is a property of an engine's eligible scope, so record it
    // once. Failure observations merged afterward retain their stronger state.
    for (id, ran) in [
        (EngineId::FileBudgets, safety),
        (
            EngineId::Suppressions,
            safety && config.anti_gaming.disallow_suppressions,
        ),
        (
            EngineId::Invariants,
            invariants && config.invariants.enforce,
        ),
        (EngineId::Complexity, complexity),
    ] {
        if ran {
            report.observe_engine(id, EngineState::Completed);
        }
    }
}
