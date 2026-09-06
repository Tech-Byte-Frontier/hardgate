use crate::config::ConfigContext;
use serde::{Deserialize, Serialize};

/// Explicit check groups; an empty selection requests every configured requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Policy,
    Format,
    Lint,
    Tests,
    Typecheck,
}

impl super::check::CheckOptions {
    pub(crate) fn selects(&self, kind: CheckKind) -> bool {
        self.checks.is_empty() || self.checks.contains(&kind)
    }
}

pub(crate) fn resolved_context(
    context: &ConfigContext,
    options: &super::check::CheckOptions,
) -> anyhow::Result<ConfigContext> {
    let mut resolved = context.clone();
    anyhow::ensure!(
        options.selects(CheckKind::Policy)
            || (options.coverage_report.is_none() && options.mutation_report.is_none()),
        "report options require the policy check group"
    );
    resolved.config.coverage.enabled |= options.coverage_report.is_some();
    resolved.config.mutation.enabled |= options.mutation_report.is_some();
    let detected = super::init::detected_orchestration(&context.root);
    let configured = &mut resolved.config.orchestration;
    if configured.format_check.is_none() {
        configured.format_check = detected.format_check;
    }
    if configured.format.is_none() {
        configured.format = detected.format;
    }
    if configured.lint.is_none() {
        configured.lint = detected.lint;
    }
    if configured.test_cmd.is_none() && configured.additional_tests.is_empty() {
        configured.test_cmd = detected.test_cmd;
        configured.additional_tests = detected.additional_tests;
    }
    if configured.typecheck.is_none() {
        configured.typecheck = detected.typecheck;
    }
    Ok(resolved)
}
