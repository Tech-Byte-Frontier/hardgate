use super::{CheckKind, CheckOptions};
use crate::{config::ConfigContext, diagnostics::GateReport, engines::OrchestrationEngine};

pub(super) fn run_orchestration(
    context: &ConfigContext,
    report: &mut GateReport,
    options: &CheckOptions,
) {
    let config = &context.config;
    let root = context.root.as_path();
    let engine = OrchestrationEngine::new(&config.orchestration);
    let policy = &config.orchestration;
    let groups = [
        (
            CheckKind::Format,
            "format_check",
            policy.format_check.iter().collect::<Vec<_>>(),
            true,
        ),
        (CheckKind::Lint, "lint", policy.lint.iter().collect(), true),
        (
            CheckKind::Tests,
            "test",
            policy
                .test_cmd
                .iter()
                .chain(&policy.additional_tests)
                .collect(),
            false,
        ),
        (
            CheckKind::Typecheck,
            "typecheck",
            policy
                .typecheck
                .iter()
                .chain(&policy.feature_checks)
                .collect(),
            false,
        ),
    ];
    let mut steps = Vec::new();
    for (kind, step, commands, required) in groups {
        if !options.selects(kind) {
            continue;
        }
        if required && commands.is_empty() {
            crate::commands::evidence::record_evidence_failure(
                report,
                true,
                crate::commands::evidence::EvidenceFailure {
                    step,
                    target: root,
                    message: format!(
                        "{} Required {step} command could not be resolved. Run `hardgate init --preview` for tool-specific setup, or configure [orchestration].",
                        if step == "format_check" {
                            "No formatter configured or detected; formatting was not evaluated."
                        } else {
                            "No linter configured or detected; lint was not evaluated."
                        }
                    ),
                },
            );
        }
        for command in commands {
            if step == "test"
                && options.evidence.is_some()
                && reuse_baseline(context, command, report)
            {
                continue;
            }
            steps.push(crate::engines::orchestration::OrchestrationStep {
                step,
                command,
                recommendation: "Resolve the reported project check before acceptance.",
            });
        }
    }
    for result in engine.run_sequence(&steps, root, config) {
        crate::commands::specialist::record(report, result, root);
    }
}

fn reuse_baseline(context: &ConfigContext, command: &str, report: &mut GateReport) -> bool {
    let Some(evidence) = crate::evidence::baseline_for(context, command, &report.evidence_runs)
    else {
        return false;
    };
    report.observe_engine(
        crate::diagnostics::execution::evidence_engine("test"),
        crate::diagnostics::execution::EngineState::Completed,
    );
    report.advisories.push(format!(
        "Reused identical authenticated test baseline `{command}` from {}",
        evidence.display()
    ));
    true
}
