use crate::config::OrchestrationConfig;
use crate::engines::process::{ProcessOutcome, append_output, run_command};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// One external tool failure (formatter, linter, or test step).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationViolation {
    pub step: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub output: String,
    pub recommendation: String,
}

#[derive(Debug, Clone)]
pub struct OrchestrationResult {
    pub step: String,
    pub command: String,
    pub success: bool,
    pub duration_ms: u128,
    pub output: String,
}

pub struct OrchestrationEngine {
    config: OrchestrationConfig,
}

pub struct OrchestrationStep<'a> {
    pub step: &'a str,
    pub command: &'a str,
    pub recommendation: &'a str,
}

impl OrchestrationEngine {
    pub fn new(config: &OrchestrationConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    pub fn has_orchestration(&self) -> bool {
        self.config.format_check.is_some()
            || self.config.format.is_some()
            || self.config.lint.is_some()
            || self.config.test_cmd.is_some()
    }

    /// Run one bounded external command with the configured timeout policy.
    /// Callers such as generated-artifact checks share the same fail-closed
    /// process handling as formatter, linter, and test steps.
    pub fn run_step(
        &self,
        spec: OrchestrationStep<'_>,
        root: &Path,
    ) -> Result<OrchestrationResult, OrchestrationViolation> {
        self.execute_step(spec, root)
    }

    pub fn run_format_check(
        &self,
        root: &Path,
    ) -> Option<Result<OrchestrationResult, OrchestrationViolation>> {
        self.config.format_check.as_ref().map(|cmd| {
            self.execute_step(
                OrchestrationStep {
                    step: "format_check",
                    command: cmd,
                    recommendation:
                        "Run `hardgate fmt` or the project formatter directly to fix formatting.",
                },
                root,
            )
        })
    }

    pub fn run_format(
        &self,
        root: &Path,
    ) -> Option<Result<OrchestrationResult, OrchestrationViolation>> {
        let cmd = self
            .config
            .format
            .as_ref()
            .or(self.config.format_check.as_ref())?;
        Some(self.execute_step(
            OrchestrationStep {
                step: "format",
                command: cmd,
                recommendation: "Format command exited with error.",
            },
            root,
        ))
    }

    pub fn run_lint(
        &self,
        root: &Path,
    ) -> Option<Result<OrchestrationResult, OrchestrationViolation>> {
        self.config.lint.as_ref().map(|cmd| {
            self.execute_step(
                OrchestrationStep {
                    step: "lint",
                    command: cmd,
                    recommendation: "Resolve linter diagnostics reported above.",
                },
                root,
            )
        })
    }

    pub fn run_tests(
        &self,
        root: &Path,
    ) -> Option<Result<OrchestrationResult, OrchestrationViolation>> {
        self.config.test_cmd.as_ref().map(|cmd| {
            self.execute_step(
                OrchestrationStep {
                    step: "test",
                    command: cmd,
                    recommendation: "Resolve the failing project tests before accepting the gate.",
                },
                root,
            )
        })
    }

    pub fn run_all_checks(
        &self,
        root: &Path,
    ) -> (Vec<OrchestrationResult>, Vec<OrchestrationViolation>) {
        let mut results = Vec::new();
        let mut violations = Vec::new();
        self.collect_step(self.run_format_check(root), &mut results, &mut violations);
        self.collect_step(self.run_lint(root), &mut results, &mut violations);
        self.collect_step(self.run_tests(root), &mut results, &mut violations);
        (results, violations)
    }

    fn collect_step(
        &self,
        result: Option<Result<OrchestrationResult, OrchestrationViolation>>,
        results: &mut Vec<OrchestrationResult>,
        violations: &mut Vec<OrchestrationViolation>,
    ) {
        match result {
            Some(Ok(result)) => results.push(result),
            Some(Err(violation)) => violations.push(violation),
            None => {}
        }
    }

    fn execute_step(
        &self,
        spec: OrchestrationStep,
        root: &Path,
    ) -> Result<OrchestrationResult, OrchestrationViolation> {
        let start = Instant::now();
        let tokens = shell_words_split(spec.command);
        if tokens.is_empty() {
            return Err(empty_command_violation(&spec));
        }
        let timeout_secs = self.timeout_secs();
        let outcome = run_command(
            &tokens,
            root,
            Duration::from_secs(timeout_secs),
            "orchestration",
        );
        finish_outcome(outcome, spec, start, timeout_secs)
    }

    fn timeout_secs(&self) -> u64 {
        self.config
            .timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .max(1)
    }
}

fn empty_command_violation(spec: &OrchestrationStep) -> OrchestrationViolation {
    OrchestrationViolation {
        step: spec.step.to_string(),
        command: spec.command.to_string(),
        exit_code: Some(1),
        output: "Empty command string; nothing was executed.".to_string(),
        recommendation: format!("Configure a non-empty {} command.", spec.step),
    }
}

fn finish_outcome(
    outcome: ProcessOutcome,
    spec: OrchestrationStep,
    start: Instant,
    timeout_secs: u64,
) -> Result<OrchestrationResult, OrchestrationViolation> {
    let duration_ms = start.elapsed().as_millis();
    match outcome {
        ProcessOutcome::Completed { status, output } if status.success() => {
            Ok(OrchestrationResult {
                step: spec.step.to_string(),
                command: spec.command.to_string(),
                success: true,
                duration_ms,
                output,
            })
        }
        ProcessOutcome::Completed { status, output } => Err(OrchestrationViolation {
            step: spec.step.to_string(),
            command: spec.command.to_string(),
            exit_code: status.code(),
            output,
            recommendation: spec.recommendation.to_string(),
        }),
        ProcessOutcome::TimedOut { output } => Err(timeout_violation(spec, output, timeout_secs)),
        ProcessOutcome::Failed { message, output } => Err(runner_violation(spec, message, output)),
    }
}

fn timeout_violation(
    spec: OrchestrationStep,
    output: String,
    timeout_secs: u64,
) -> OrchestrationViolation {
    OrchestrationViolation {
        step: spec.step.to_string(),
        command: spec.command.to_string(),
        exit_code: None,
        output: append_output(
            output,
            format!("Command timed out after {timeout_secs}s; process group terminated."),
        ),
        recommendation: format!(
            "Fix the {} command or raise orchestration.timeout_secs above {timeout_secs} only when the longer runtime is expected.",
            spec.step
        ),
    }
}

fn runner_violation(
    spec: OrchestrationStep,
    message: String,
    output: String,
) -> OrchestrationViolation {
    OrchestrationViolation {
        step: spec.step.to_string(),
        command: spec.command.to_string(),
        exit_code: None,
        output: append_output(output, message),
        recommendation: format!(
            "Ensure the {} command is installed, executable, and valid for this project.",
            spec.step
        ),
    }
}

pub fn shell_words_split(cmd: &str) -> Vec<String> {
    // Quote-aware split so `--exact "my test"` and spaced paths survive.
    let mut lexer = ShellLexer::default();
    for c in cmd.chars() {
        lexer.feed(c);
    }
    lexer.finish()
}

#[derive(Default)]
struct ShellLexer {
    out: Vec<String>,
    cur: String,
    single: bool,
    double: bool,
    escaped: bool,
    has_token: bool,
}

impl ShellLexer {
    fn feed(&mut self, c: char) {
        if self.escaped {
            self.push_char(c);
            self.escaped = false;
            return;
        }
        if c == '\\' && !self.single {
            self.escaped = true;
            return;
        }
        if self.handle_quote(c) {
            return;
        }
        if c.is_whitespace() && !self.single && !self.double {
            self.flush_token();
        } else {
            self.push_char(c);
        }
    }

    /// Returns true if `c` was a quote toggle.
    fn handle_quote(&mut self, c: char) -> bool {
        if c == '\'' && !self.double {
            self.single = !self.single;
            self.has_token = true;
            true
        } else if c == '"' && !self.single {
            self.double = !self.double;
            self.has_token = true;
            true
        } else {
            false
        }
    }

    fn push_char(&mut self, c: char) {
        self.cur.push(c);
        self.has_token = true;
    }

    fn flush_token(&mut self) {
        if self.has_token {
            self.out.push(std::mem::take(&mut self.cur));
            self.has_token = false;
        }
    }

    fn finish(mut self) -> Vec<String> {
        self.flush_token();
        self.out
    }
}
