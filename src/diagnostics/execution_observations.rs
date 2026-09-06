use super::GateReport;
use super::execution::{EngineId, EngineState, evidence_engine, observe};

impl GateReport {
    pub(crate) fn observe_engine(&mut self, id: EngineId, state: EngineState) {
        observe(&mut self.engine_observations, id, state);
    }

    pub(crate) fn observe_evidence_failure(&mut self, step: &str, message: &str) {
        if matches!(step, "read-source" | "classify-source") {
            for id in [
                EngineId::FileBudgets,
                EngineId::Suppressions,
                EngineId::Complexity,
                EngineId::Invariants,
                EngineId::Clones,
            ] {
                self.observe_incomplete(id, step, message);
            }
        } else {
            self.observe_incomplete(evidence_engine(step), step, message);
        }
    }

    fn observe_incomplete(&mut self, id: EngineId, step: &str, message: &str) {
        self.observe_engine(id, EngineState::Incomplete);
        self.engine_reasons
            .entry(id)
            .or_insert_with(|| format!("{step}: {message}"));
    }

    pub(super) fn finalize_execution(&mut self) {
        if self.tool_findings_count() > 0 {
            self.observe_engine(EngineId::Lint, EngineState::Failed);
        }
        for (id, count) in [
            (EngineId::FileBudgets, self.budget_violations.len()),
            (EngineId::Suppressions, self.suppression_violations.len()),
            (EngineId::Complexity, self.complexity_violations.len()),
            (EngineId::Invariants, self.invariant_violations.len()),
            (EngineId::Clones, self.clone_violations.len()),
            (EngineId::MutationReport, self.mutation_violations.len()),
        ] {
            if count > 0 {
                self.observe_engine(id, EngineState::Failed);
            }
        }
        for violation in &self.orchestration_violations {
            let state = if violation.exit_code.is_none()
                || matches!(violation.exit_code, Some(126 | 127))
            {
                EngineState::Incomplete
            } else {
                EngineState::Failed
            };
            observe(
                &mut self.engine_observations,
                evidence_engine(&violation.step),
                state,
            );
        }
        for violation in &self.coverage_violations {
            let state = if missing_coverage(&violation.metric) {
                EngineState::Incomplete
            } else {
                EngineState::Failed
            };
            observe(&mut self.engine_observations, EngineId::Coverage, state);
        }
        if let Some(plan) = &mut self.execution {
            plan.reconcile(&self.engine_observations, &self.engine_reasons);
        }
    }
}

pub(crate) fn missing_coverage(metric: &str) -> bool {
    matches!(
        metric,
        "Missing Source Coverage"
            | "Missing Diff Coverage"
            | "Missing Critical Path"
            | "Coverage Count Overflow"
    )
}
