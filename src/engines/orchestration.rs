use crate::config::OrchestrationConfig;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

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

struct StepSpec<'a> {
    step: &'a str,
    cmd_str: &'a str,
    recommendation: &'a str,
}

impl OrchestrationEngine {
    pub fn new(config: &OrchestrationConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    pub fn has_orchestration(&self) -> bool {
        self.config.format_check.is_some() || self.config.lint.is_some()
    }

    pub fn run_format_check(
        &self,
        root: &Path,
    ) -> Option<Result<OrchestrationResult, OrchestrationViolation>> {
        self.config.format_check.as_ref().map(|cmd| {
            self.execute_step(
                StepSpec {
                    step: "format_check",
                    cmd_str: cmd,
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
            StepSpec {
                step: "format",
                cmd_str: cmd,
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
                StepSpec {
                    step: "lint",
                    cmd_str: cmd,
                    recommendation: "Resolve linter diagnostics reported above.",
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

        (results, violations)
    }

    fn collect_step(
        &self,
        res: Option<Result<OrchestrationResult, OrchestrationViolation>>,
        results: &mut Vec<OrchestrationResult>,
        violations: &mut Vec<OrchestrationViolation>,
    ) {
        let Some(res) = res else { return };
        match res {
            Ok(ok) => results.push(ok),
            Err(err) => violations.push(err),
        }
    }

    fn execute_step(
        &self,
        spec: StepSpec,
        root: &Path,
    ) -> Result<OrchestrationResult, OrchestrationViolation> {
        let start = Instant::now();
        let tokens = shell_words_split(spec.cmd_str);
        if tokens.is_empty() {
            return Err(empty_command_violation(&spec));
        }

        let mut cmd = build_command(&tokens, root);

        match cmd.output() {
            Ok(output) => finish_output(output, &spec, start),
            Err(e) => Err(spawn_failure_violation(&spec, &tokens[0], &e)),
        }
    }
}

fn empty_command_violation(spec: &StepSpec) -> OrchestrationViolation {
    OrchestrationViolation {
        step: spec.step.to_string(),
        command: spec.cmd_str.to_string(),
        exit_code: Some(1),
        output: "Empty command string".to_string(),
        recommendation: spec.recommendation.to_string(),
    }
}

fn build_command(tokens: &[String], root: &Path) -> Command {
    let mut cmd = Command::new(&tokens[0]);
    cmd.args(&tokens[1..]);
    cmd.current_dir(root);
    prepend_local_bin(&mut cmd, root);
    cmd
}

fn prepend_local_bin(cmd: &mut Command, root: &Path) {
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = std::env::split_paths(&current_path).collect::<Vec<_>>();
    let local_bin = root.join("node_modules").join(".bin");
    if local_bin.exists() {
        paths.insert(0, local_bin);
    }
    if let Ok(new_path) = std::env::join_paths(paths) {
        cmd.env("PATH", new_path);
    }
}

fn finish_output(
    output: std::process::Output,
    spec: &StepSpec,
    start: Instant,
) -> Result<OrchestrationResult, OrchestrationViolation> {
    let duration_ms = start.elapsed().as_millis();
    let combined = combine_output(&output);
    if output.status.success() {
        Ok(OrchestrationResult {
            step: spec.step.to_string(),
            command: spec.cmd_str.to_string(),
            success: true,
            duration_ms,
            output: combined,
        })
    } else {
        Err(OrchestrationViolation {
            step: spec.step.to_string(),
            command: spec.cmd_str.to_string(),
            exit_code: output.status.code(),
            output: combined,
            recommendation: spec.recommendation.to_string(),
        })
    }
}

fn combine_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        stderr
    } else {
        format!("{}\n{}", stdout, stderr)
    };
    combined.trim().to_string()
}

fn spawn_failure_violation(
    spec: &StepSpec,
    program: &str,
    e: &std::io::Error,
) -> OrchestrationViolation {
    OrchestrationViolation {
        step: spec.step.to_string(),
        command: spec.cmd_str.to_string(),
        exit_code: None,
        output: format!("Failed to execute '{}': {}", program, e),
        recommendation: format!(
            "Ensure '{}' is installed in project dependencies (e.g., package.json) or global PATH.",
            program
        ),
    }
}

pub fn shell_words_split(cmd: &str) -> Vec<String> {
    // Quote-aware split so `--exact "my test"` and spaced paths survive.
    let mut lex = ShellLexer::default();
    for c in cmd.chars() {
        lex.feed(c);
    }
    lex.finish()
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
