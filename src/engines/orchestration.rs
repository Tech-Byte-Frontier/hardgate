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

        if let Some(res) = self.run_format_check(root) {
            match res {
                Ok(ok) => results.push(ok),
                Err(err) => violations.push(err),
            }
        }

        if let Some(res) = self.run_lint(root) {
            match res {
                Ok(ok) => results.push(ok),
                Err(err) => violations.push(err),
            }
        }

        (results, violations)
    }

    fn execute_step(
        &self,
        spec: StepSpec,
        root: &Path,
    ) -> Result<OrchestrationResult, OrchestrationViolation> {
        let start = Instant::now();
        let tokens = shell_words_split(spec.cmd_str);
        if tokens.is_empty() {
            return Err(OrchestrationViolation {
                step: spec.step.to_string(),
                command: spec.cmd_str.to_string(),
                exit_code: Some(1),
                output: "Empty command string".to_string(),
                recommendation: spec.recommendation.to_string(),
            });
        }

        let program = &tokens[0];
        let args = &tokens[1..];

        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.current_dir(root);

        // Prepend ./node_modules/.bin to PATH for local project binaries (e.g. oxfmt, oxlint, biome, prettier)
        let current_path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = std::env::split_paths(&current_path).collect::<Vec<_>>();
        let local_bin = root.join("node_modules").join(".bin");
        if local_bin.exists() {
            paths.insert(0, local_bin);
        }
        if let Ok(new_path) = std::env::join_paths(paths) {
            cmd.env("PATH", new_path);
        }

        match cmd.output() {
            Ok(output) => {
                let duration_ms = start.elapsed().as_millis();
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let combined = if stderr.is_empty() {
                    stdout
                } else if stdout.is_empty() {
                    stderr
                } else {
                    format!("{}\n{}", stdout, stderr)
                };

                if output.status.success() {
                    Ok(OrchestrationResult {
                        step: spec.step.to_string(),
                        command: spec.cmd_str.to_string(),
                        success: true,
                        duration_ms,
                        output: combined.trim().to_string(),
                    })
                } else {
                    Err(OrchestrationViolation {
                        step: spec.step.to_string(),
                        command: spec.cmd_str.to_string(),
                        exit_code: output.status.code(),
                        output: combined.trim().to_string(),
                        recommendation: spec.recommendation.to_string(),
                    })
                }
            }
            Err(e) => Err(OrchestrationViolation {
                step: spec.step.to_string(),
                command: spec.cmd_str.to_string(),
                exit_code: None,
                output: format!("Failed to execute '{}': {}", program, e),
                recommendation: format!(
                    "Ensure '{}' is installed in project dependencies (e.g., package.json) or global PATH.",
                    program
                ),
            }),
        }
    }
}

fn shell_words_split(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().map(|s| s.to_string()).collect()
}
