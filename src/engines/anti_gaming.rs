use crate::config::AntiGamingConfig;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuppressionViolation {
    pub file: PathBuf,
    pub line_number: usize,
    pub token: String,
    pub line_content: String,
    pub message: String,
}

pub struct AntiGamingScanner {
    patterns: Vec<Regex>,
    custom_patterns: Vec<Regex>,
}

struct LineContext<'a> {
    file: &'a Path,
    line_num: usize,
    line: &'a str,
}

impl AntiGamingScanner {
    pub fn new(config: &AntiGamingConfig) -> Self {
        let standard_patterns = vec![
            r"@ts-ignore",
            r"@ts-nocheck",
            r"eslint-disable",
            r"oxlint-disable",
            r"prettier-ignore",
            r"#\[allow\(",
            r"#\[expect\(",
            r"#\!\[allow\(",
            r"mutants::skip",
            r"coverage\(off\)",
            r"#\s*type:\s*ignore",
            r"#\s*noqa",
            r"#\s*pragma:\s*no\s*cover",
            r"c8\s+ignore",
            r"istanbul\s+ignore",
            r"v8\s+ignore",
        ];

        let patterns = standard_patterns
            .into_iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();

        let custom_patterns = config
            .custom_forbidden_tokens
            .iter()
            .filter_map(|t| Regex::new(&regex::escape(t)).ok())
            .collect();

        Self {
            patterns,
            custom_patterns,
        }
    }

    pub fn scan_content(
        &self,
        path: &Path,
        content: &str,
        root: &Path,
    ) -> Vec<SuppressionViolation> {
        let mut violations = Vec::new();
        let rel_path = path.strip_prefix(root).unwrap_or(path);

        for (idx, line) in content.lines().enumerate() {
            let ctx = LineContext {
                file: rel_path,
                line_num: idx + 1,
                line: line.trim(),
            };

            if check_patterns(&self.patterns, &ctx, false, &mut violations) {
                continue;
            }
            check_patterns(&self.custom_patterns, &ctx, true, &mut violations);
        }

        violations
    }
}

fn check_patterns(
    patterns: &[Regex],
    ctx: &LineContext,
    is_custom: bool,
    violations: &mut Vec<SuppressionViolation>,
) -> bool {
    for re in patterns {
        if let Some(mat) = re.find(ctx.line) {
            if is_valid_suppression_context(ctx.line, mat.start(), mat.as_str()) {
                let msg = if is_custom {
                    format!("Anti-gaming: forbidden token '{}'", mat.as_str())
                } else {
                    format!(
                        "Anti-gaming: suppression pragma '{}' prohibited",
                        mat.as_str()
                    )
                };
                violations.push(SuppressionViolation {
                    file: ctx.file.to_path_buf(),
                    line_number: ctx.line_num,
                    token: mat.as_str().to_string(),
                    line_content: ctx.line.to_string(),
                    message: msg,
                });
                return true;
            }
        }
    }
    false
}

fn is_valid_suppression_context(line: &str, match_start: usize, token: &str) -> bool {
    let prefix = &line[..match_start];

    if token.starts_with("#[") || token.starts_with("#![") {
        return check_rust_attr_prefix(prefix);
    }

    let in_string = prefix.ends_with("r\"")
        || (prefix.contains('"') && !prefix.contains("//") && !prefix.contains('#'));
    if in_string {
        return false;
    }

    if token.starts_with('#') {
        return true;
    }

    prefix.contains("//") || prefix.contains("/*") || prefix.trim_start().starts_with('*')
}

fn check_rust_attr_prefix(prefix: &str) -> bool {
    prefix.trim().is_empty() || prefix.trim_end().ends_with(';') || prefix.contains("//")
}
