use super::GateReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterEngine {
    Complexity,
    Budget,
    Suppression,
    Invariant,
    Clone,
    Coverage,
    Mutation,
    Orchestration,
    Specialist,
}

fn parse_static_engine(norm: &str) -> Option<FilterEngine> {
    match norm {
        "complexity" => Some(FilterEngine::Complexity),
        "budget" | "file-budget" | "file-budgets" => Some(FilterEngine::Budget),
        "suppression" | "suppressions" | "anti-gaming" => Some(FilterEngine::Suppression),
        "invariant" | "invariants" => Some(FilterEngine::Invariant),
        "clone" | "clones" => Some(FilterEngine::Clone),
        _ => None,
    }
}

fn parse_verification_engine(norm: &str) -> Option<FilterEngine> {
    match norm {
        "coverage" => Some(FilterEngine::Coverage),
        "mutation" | "mutation-report" => Some(FilterEngine::Mutation),
        "orchestration" | "tool" => Some(FilterEngine::Orchestration),
        "specialist" | "clippy" | "rustc" => Some(FilterEngine::Specialist),
        _ => None,
    }
}

pub fn filter_by_engine(report: &mut GateReport, engine: &str) -> anyhow::Result<()> {
    let target = parse_engine(engine)?;
    retain_static_violations(report, target);
    retain_verification_violations(report, target);
    Ok(())
}

pub fn parse_engine(engine: &str) -> anyhow::Result<FilterEngine> {
    let norm = engine.to_ascii_lowercase().replace('_', "-");
    let target = parse_static_engine(&norm).or_else(|| parse_verification_engine(&norm));
    target.ok_or_else(|| anyhow::anyhow!("Unknown report engine `{engine}`; use complexity, budget, suppression, invariant, clones, coverage, mutation, orchestration, or specialist"))
}

fn retain_static_violations(report: &mut GateReport, target: FilterEngine) {
    if target != FilterEngine::Complexity {
        report.complexity_violations.clear();
    }
    if target != FilterEngine::Budget {
        report.budget_violations.clear();
    }
    if target != FilterEngine::Suppression {
        report.suppression_violations.clear();
    }
    if target != FilterEngine::Invariant {
        report.invariant_violations.clear();
    }
    if target != FilterEngine::Clone {
        report.clone_violations.clear();
    }
}

fn retain_verification_violations(report: &mut GateReport, target: FilterEngine) {
    if target != FilterEngine::Specialist {
        report.tool_diagnostics.clear();
    }
    if target != FilterEngine::Coverage {
        report.coverage_violations.clear();
    }
    if target != FilterEngine::Mutation {
        report.mutation_violations.clear();
    }

    if target != FilterEngine::Orchestration {
        report.orchestration_violations.clear();
    }
}

pub(crate) fn retain_engine(report: &mut GateReport, target: FilterEngine) {
    retain_static_violations(report, target);
    retain_verification_violations(report, target);
}
