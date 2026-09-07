//! Read-only preflight: resolve launchers and validate existing evidence without running tools.
use super::{CommandOutcome, CommandResult, outcome::write_stdout};
use crate::config::ConfigContext;
use crate::evidence::EvidenceKind;
use serde::Serialize;
mod tools;
use tools::tool_check;

#[derive(Serialize)]
struct Check {
    name: String,
    ready: bool,
    detail: String,
}

pub fn cmd_doctor(context: &ConfigContext, json: bool) -> CommandResult {
    let context =
        &super::check_selection::resolved_context(context, &super::CheckOptions::default())?;
    let mut checks = tool_checks(context);
    evidence_checks(context, &mut checks);
    let ready = checks.iter().all(|check| check.ready);
    let host = if cfg!(target_os = "linux") {
        "Linux host: evidence production requires successful resource and filesystem isolation admission at runtime."
    } else {
        "This Mac supports native checks; use a Linux host for evidence production (hardgate evidence)."
    };
    let report = serde_json::json!({"command": "doctor", "schema_version": 1, "ready": ready, "host": host, "checks": checks, "note": "Preflight checks launcher availability and existing source-bound evidence; it does not execute project tools or establish acceptance."});
    if json {
        write_stdout(&format!("{}\n", serde_json::to_string_pretty(&report)?))?;
    } else {
        let mut text = format!(
            "Hardgate doctor: {}\n{host}\n",
            if ready {
                "preflight ready"
            } else {
                "setup or evidence incomplete"
            }
        );
        for check in checks
            .iter()
            .filter(|check| !check.ready)
            .chain(checks.iter().filter(|check| check.ready))
        {
            text.push_str(&format!(
                "{} {}: {}\n",
                if check.ready { "ready" } else { "missing" },
                check.name,
                check.detail
            ));
        }
        text.push_str("Launcher presence does not prove tool/script readiness. No project commands were executed; run hardgate check for acceptance.\n");
        write_stdout(&text)?;
    }
    Ok(if ready {
        CommandOutcome::Passed
    } else {
        CommandOutcome::Incomplete
    })
}

fn tool_checks(context: &ConfigContext) -> Vec<Check> {
    let config = &context.config.orchestration;
    let mut checks = Vec::new();
    for (name, command) in [
        ("format", &config.format),
        ("format_check", &config.format_check),
        ("format_files", &config.format_files),
        ("format_check_files", &config.format_check_files),
        ("lint", &config.lint),
        ("tests", &config.test_cmd),
        ("typecheck", &config.typecheck),
        (
            "generated freshness",
            &context.config.generated.freshness_command,
        ),
    ] {
        if let Some(command) = command {
            checks.push(tool_check(name, command, &context.root));
        }
    }
    for (name, command) in [
        ("format_check", &config.format_check),
        ("lint", &config.lint),
    ] {
        if command.is_none() {
            checks.push(Check { name: name.into(), ready: false, detail: format!("No command configured or detected; set [orchestration].{name} before hardgate check.") });
        }
    }
    for command in config.additional_tests.iter().chain(&config.feature_checks) {
        checks.push(tool_check("additional check", command, &context.root));
    }
    if config.require_isolation && !cfg!(target_os = "linux") {
        checks.push(Check {
            name: "required isolation".into(),
            ready: false,
            detail: "Use a Linux host for orchestration.require_isolation=true.".into(),
        });
    }
    checks
}

fn evidence_checks(context: &ConfigContext, checks: &mut Vec<Check>) {
    if context.config.coverage.enabled {
        checks.push(evidence_check(
            context,
            "coverage",
            context.config.coverage.report.as_deref(),
            EvidenceKind::Coverage,
        ));
    }
    if context.config.mutation.enabled {
        match context
            .config
            .mutation
            .reports
            .as_deref()
            .filter(|reports| !reports.is_empty())
        {
            Some(reports) => {
                for report in reports {
                    checks.push(evidence_check(
                        context,
                        "mutation",
                        Some(report),
                        EvidenceKind::Mutation,
                    ));
                }
            }
            None => checks.push(evidence_check(
                context,
                "mutation",
                None,
                EvidenceKind::Mutation,
            )),
        }
    }
}

fn evidence_check(
    context: &ConfigContext,
    name: &str,
    path: Option<&str>,
    kind: EvidenceKind,
) -> Check {
    let result = path
        .ok_or_else(|| anyhow::anyhow!("report path is not configured"))
        .and_then(|path| {
            crate::evidence::verify(
                &context.root,
                &context.root.join(path),
                kind,
                &context.config,
            )
        });
    Check { name: format!("{name} evidence"), ready: result.is_ok(), detail: result.map_or_else(|error| format!("{error:#}; run hardgate evidence on Linux, then configure {name} report paths"), |()| format!("{} has a current source-bound receipt; policy thresholds are evaluated by hardgate check", path.unwrap_or_default())) }
}
