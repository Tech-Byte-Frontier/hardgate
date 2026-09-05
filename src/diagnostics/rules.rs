use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, CoverageViolation, DeadCodeViolation,
    InvariantViolation, MutationViolation, OrchestrationViolation, SuppressionViolation,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const BUDGET: &str = "budget";
const SUPPRESSION: &str = "suppression";
const COMPLEXITY: &str = "complexity";
const INVARIANT: &str = "invariant";
const CLONE: &str = "clone";
const COVERAGE: &str = "coverage";
const MUTATION: &str = "mutation";
const DEAD_CODE: &str = "dead-code";
const ORCHESTRATION: &str = "orchestration";

/// Stable machine-readable explanation of one blocking gate finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleDiagnostic {
    pub rule_id: String,
    pub category: String,
    pub message: String,
    pub locations: Vec<DiagnosticLocation>,
    pub recommendation: String,
}

/// A source or evidence location attached to a [`RuleDiagnostic`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticLocation {
    pub file: PathBuf,
    pub line: Option<usize>,
    pub end_line: Option<usize>,
}

/// Convert every blocking array in a report to a deterministic rule stream.
///
/// The category order is part of the output contract. Findings retain the
/// order in each report array, while rule IDs never include paths, line
/// numbers, or mutable human-readable messages.
pub fn diagnostics(report: &super::GateReport) -> Vec<RuleDiagnostic> {
    let mut diagnostics = Vec::with_capacity(report.total_violations());
    diagnostics.extend(report.budget_violations.iter().map(budget_diagnostic));
    diagnostics.extend(
        report
            .suppression_violations
            .iter()
            .map(suppression_diagnostic),
    );
    diagnostics.extend(
        report
            .complexity_violations
            .iter()
            .map(complexity_diagnostic),
    );
    diagnostics.extend(report.invariant_violations.iter().map(invariant_diagnostic));
    diagnostics.extend(report.clone_violations.iter().map(clone_diagnostic));
    diagnostics.extend(report.coverage_violations.iter().map(coverage_diagnostic));
    diagnostics.extend(report.mutation_violations.iter().map(mutation_diagnostic));
    diagnostics.extend(report.dead_code_violations.iter().map(dead_code_diagnostic));
    diagnostics.extend(
        report
            .orchestration_violations
            .iter()
            .map(orchestration_diagnostic),
    );
    diagnostics
}

fn budget_diagnostic(violation: &BudgetViolation) -> RuleDiagnostic {
    let recommendation = match violation.metric.as_str() {
        "File Byte Size" => {
            "Split the file into cohesive modules or remove unnecessary content before accepting the gate."
        }
        metric if is_physical_lines_metric(metric) => {
            "Split the file into cohesive modules or remove unnecessary lines before accepting the gate."
        }
        _ => "Reduce the measured file size while keeping the configured policy intact.",
    };
    diagnostic(
        (BUDGET, budget_rule_id(&violation.metric)),
        violation.message.clone(),
        vec![location(&violation.file, None, None)],
        recommendation.to_string(),
    )
}

fn suppression_diagnostic(violation: &SuppressionViolation) -> RuleDiagnostic {
    diagnostic(
        (SUPPRESSION, "HG-SUPPRESSION-FORBIDDEN"),
        violation.message.clone(),
        vec![location(&violation.file, line(violation.line_number), None)],
        "Remove the suppression directive and fix the underlying compiler, linter, or coverage finding."
            .to_string(),
    )
}

fn complexity_diagnostic(violation: &ComplexityViolation) -> RuleDiagnostic {
    diagnostic(
        (COMPLEXITY, complexity_rule_id(&violation.metric)),
        violation.message.clone(),
        vec![location(
            &violation.file,
            line(violation.line_number),
            line(violation.end_line),
        )],
        recommendation_or(
            &violation.recommendation,
            "Refactor the function into smaller, focused units while preserving its behavior.",
        ),
    )
}

fn invariant_diagnostic(violation: &InvariantViolation) -> RuleDiagnostic {
    let recommendation = match violation.violation_type.as_str() {
        "Disallowed Import" => {
            "Remove or invert the disallowed import to satisfy the architectural rule."
        }
        "Disallowed Call" => "Replace the disallowed call with an allowed boundary or abstraction.",
        "Disallowed Token" => {
            "Remove the disallowed token or change the implementation to satisfy the architectural rule."
        }
        _ => "Change the offending target to satisfy the configured architectural invariant.",
    };
    diagnostic(
        (INVARIANT, invariant_rule_id(&violation.violation_type)),
        violation.message.clone(),
        vec![location(&violation.file, line(violation.line_number), None)],
        recommendation.to_string(),
    )
}

fn clone_diagnostic(violation: &CloneViolation) -> RuleDiagnostic {
    diagnostic(
        (CLONE, "HG-CLONE-DUPLICATE-BLOCK"),
        violation.message.clone(),
        vec![
            location(
                &violation.file_a,
                line(violation.lines_a.0),
                line(violation.lines_a.1),
            ),
            location(
                &violation.file_b,
                line(violation.lines_b.0),
                line(violation.lines_b.1),
            ),
        ],
        recommendation_or(
            &violation.recommendation,
            "Extract the duplicated logic into a shared helper while preserving behavior.",
        ),
    )
}

fn coverage_diagnostic(violation: &CoverageViolation) -> RuleDiagnostic {
    diagnostic(
        (COVERAGE, coverage_rule_id(&violation.metric)),
        violation.message.clone(),
        vec![location(&violation.file, None, None)],
        recommendation_or(
            &violation.recommendation,
            "Regenerate coverage evidence and add tests for the uncovered behavior.",
        ),
    )
}

fn mutation_diagnostic(violation: &MutationViolation) -> RuleDiagnostic {
    diagnostic(
        (MUTATION, mutation_rule_id(&violation.metric)),
        violation.message.clone(),
        vec![location(&violation.report_file, None, None)],
        recommendation_or(
            &violation.recommendation,
            "Repair the mutation evidence and add semantic tests for the affected behavior.",
        ),
    )
}

fn dead_code_diagnostic(violation: &DeadCodeViolation) -> RuleDiagnostic {
    diagnostic(
        (DEAD_CODE, dead_code_rule_id(&violation.violation_type)),
        violation.message.clone(),
        vec![location(
            &violation.file,
            violation.line_number.and_then(line),
            None,
        )],
        recommendation_or(
            &violation.recommendation,
            "Remove the unused code or connect it to an active entry point.",
        ),
    )
}

fn orchestration_diagnostic(violation: &OrchestrationViolation) -> RuleDiagnostic {
    let message = if violation.output.trim().is_empty() {
        format!("Orchestration step `{}` failed.", violation.step)
    } else {
        violation.output.clone()
    };
    diagnostic(
        (ORCHESTRATION, orchestration_rule_id(&violation.step)),
        message,
        Vec::new(),
        recommendation_or(
            &violation.recommendation,
            "Resolve the failing orchestration step before accepting the gate.",
        ),
    )
}

fn diagnostic(
    (category, rule_id): (&str, &str),
    message: String,
    locations: Vec<DiagnosticLocation>,
    recommendation: String,
) -> RuleDiagnostic {
    RuleDiagnostic {
        rule_id: rule_id.to_string(),
        category: category.to_string(),
        message,
        locations,
        recommendation,
    }
}

fn location(file: &Path, line: Option<usize>, end_line: Option<usize>) -> DiagnosticLocation {
    DiagnosticLocation {
        file: file.to_path_buf(),
        line,
        end_line,
    }
}

fn line(value: usize) -> Option<usize> {
    (value > 0).then_some(value)
}

fn recommendation_or(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn is_physical_lines_metric(metric: &str) -> bool {
    metric
        .strip_prefix("Physical Lines (.")
        .and_then(|suffix| suffix.strip_suffix(')'))
        .is_some()
}

fn budget_rule_id(metric: &str) -> &'static str {
    if metric == "File Byte Size" {
        "HG-BUDGET-FILE-BYTE-SIZE"
    } else if is_physical_lines_metric(metric) {
        "HG-BUDGET-PHYSICAL-LINES"
    } else {
        "HG-BUDGET-UNKNOWN-METRIC"
    }
}

fn complexity_rule_id(metric: &str) -> &'static str {
    lookup_rule_id(metric, COMPLEXITY_IDS, "HG-COMPLEXITY-UNKNOWN-METRIC")
}

fn invariant_rule_id(kind: &str) -> &'static str {
    lookup_rule_id(kind, INVARIANT_IDS, "HG-INVARIANT-UNKNOWN-KIND")
}

fn coverage_rule_id(metric: &str) -> &'static str {
    lookup_rule_id(metric, COVERAGE_IDS, "HG-COVERAGE-UNKNOWN-METRIC")
}

fn mutation_rule_id(metric: &str) -> &'static str {
    lookup_rule_id(metric, MUTATION_IDS, "HG-MUTATION-UNKNOWN-METRIC")
}

fn dead_code_rule_id(kind: &str) -> &'static str {
    lookup_rule_id(kind, DEAD_CODE_IDS, "HG-DEAD-CODE-UNKNOWN-KIND")
}

fn orchestration_rule_id(step: &str) -> &'static str {
    lookup_rule_id(step, ORCHESTRATION_IDS, "HG-ORCHESTRATION-UNKNOWN-STEP")
}

fn lookup_rule_id(value: &str, table: &'static str, fallback: &'static str) -> &'static str {
    let mut entries = table.split('\0');
    while let Some(key) = entries.next() {
        let Some(rule_id) = entries.next() else {
            break;
        };
        if key == value {
            return rule_id;
        }
    }
    fallback
}

const COMPLEXITY_IDS: &str = concat!(
    "Cyclomatic Complexity\0HG-COMPLEXITY-CYCLOMATIC\0",
    "Cognitive Complexity\0HG-COMPLEXITY-COGNITIVE\0",
    "Parameter Count\0HG-COMPLEXITY-PARAMETERS\0",
    "Function Lines\0HG-COMPLEXITY-FUNCTION-LINES\0",
    "Nesting Depth\0HG-COMPLEXITY-NESTING\0",
    "Halstead Difficulty\0HG-COMPLEXITY-HALSTEAD\0",
    "Statement Count\0HG-COMPLEXITY-STATEMENTS\0",
    "ABC Score\0HG-COMPLEXITY-ABC\0",
);

const INVARIANT_IDS: &str = concat!(
    "Disallowed Import\0HG-INVARIANT-DISALLOWED-IMPORT\0",
    "Disallowed Call\0HG-INVARIANT-DISALLOWED-CALL\0",
    "Disallowed Token\0HG-INVARIANT-DISALLOWED-TOKEN\0",
);

const COVERAGE_IDS: &str = concat!(
    "Coverage Count Overflow\0HG-COVERAGE-COUNT-OVERFLOW\0",
    "Global Line Coverage\0HG-COVERAGE-GLOBAL-LINES\0",
    "Global Function Coverage\0HG-COVERAGE-GLOBAL-FUNCTIONS\0",
    "Global Branch Coverage\0HG-COVERAGE-GLOBAL-BRANCHES\0",
    "Missing Source Coverage\0HG-COVERAGE-MISSING-SOURCE\0",
    "CRAP Score\0HG-COVERAGE-CRAP\0",
    "Missing Critical Path\0HG-COVERAGE-MISSING-CRITICAL-PATH\0",
    "Critical Path 100% Coverage\0HG-COVERAGE-CRITICAL-PATH\0",
    "Missing Diff Coverage\0HG-COVERAGE-MISSING-DIFF\0",
    "Diff Line Coverage\0HG-COVERAGE-DIFF-LINES\0",
);

const MUTATION_IDS: &str = concat!(
    "Mutation Kill Rate\0HG-MUTATION-KILL-RATE\0",
    "Mutation Timeouts\0HG-MUTATION-TIMEOUTS\0",
    "Mutation Compile Errors\0HG-MUTATION-COMPILE-ERRORS\0",
    "Mutation Runner Errors\0HG-MUTATION-RUNNER-ERRORS\0",
    "Mutation Unviable Mutants\0HG-MUTATION-UNVIABLE\0",
);

const DEAD_CODE_IDS: &str = concat!(
    "Unreferenced File\0HG-DEAD-CODE-UNREFERENCED-FILE\0",
    "Unused Export\0HG-DEAD-CODE-UNUSED-EXPORT\0",
);

const ORCHESTRATION_IDS: &str = concat!(
    "format_check\0HG-ORCHESTRATION-FORMAT-CHECK\0",
    "format\0HG-ORCHESTRATION-FORMAT\0",
    "lint\0HG-ORCHESTRATION-LINT\0",
    "test\0HG-ORCHESTRATION-TEST\0",
    "generated-freshness\0HG-ORCHESTRATION-GENERATED-FRESHNESS\0",
    "coverage-diff\0HG-ORCHESTRATION-COVERAGE-DIFF\0",
    "coverage-report\0HG-ORCHESTRATION-COVERAGE-REPORT\0",
    "coverage-source-classification\0HG-ORCHESTRATION-COVERAGE-SOURCE-CLASSIFICATION\0",
    "dead-code-context\0HG-ORCHESTRATION-DEAD-CODE-CONTEXT\0",
    "read-clone-index\0HG-ORCHESTRATION-READ-CLONE-INDEX\0",
    "clone-index\0HG-ORCHESTRATION-CLONE-INDEX\0",
    "read-source\0HG-ORCHESTRATION-READ-SOURCE\0",
    "parse-source\0HG-ORCHESTRATION-PARSE-SOURCE\0",
    "classify-source\0HG-ORCHESTRATION-CLASSIFY-SOURCE\0",
    "unsupported-source\0HG-ORCHESTRATION-UNSUPPORTED-SOURCE\0",
    "mutation-report\0HG-ORCHESTRATION-MUTATION-REPORT\0",
    "legacy-ratchet\0HG-ORCHESTRATION-LEGACY-RATCHET\0",
);

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
