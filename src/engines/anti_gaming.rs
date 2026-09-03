use crate::config::AntiGamingConfig;
use crate::engines::util::is_inside_string;
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
            r"@ts-expect-error",
            r"@ts-nocheck",
            r"eslint-disable",
            r"eslint-next-line",
            r"oxlint-disable",
            r"biome-ignore",
            r"prettier-ignore",
            r"#\[allow\(",
            r"#\[expect\(",
            r"#\!\[allow\(",
            r"mutants::skip",
            r"coverage\(off\)",
            r"#\s*type:\s*ignore",
            r"#\s*noqa",
            r"ruff:\s*noqa",
            r"#\s*pragma:\s*no\s*cover",
            r"c8\s+ignore",
            r"istanbul\s+ignore",
            r"v8\s+ignore",
            r"istanbul\s+ignore\s+(next|if|else)",
            r"sonarlint-disable",
            r"nosonar",
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

    // Skip occurrences inside string literals (data, not pragmas).
    if is_inside_string(prefix) {
        return false;
    }

    if token.starts_with('#') {
        // Hash-directives live in comments by definition, including trailing
        // code-plus-comment lines, so flag them once strings are excluded.
        return true;
    }

    is_comment_context(line, prefix)
}

fn is_comment_context(line: &str, prefix: &str) -> bool {
    let trimmed_prefix = prefix.trim_start();
    let trimmed_line = line.trim_start();
    prefix.contains("//")
        || prefix.contains("/*")
        || prefix.contains('#')
        || line.contains("/*")
        || trimmed_prefix.starts_with('*')
        || trimmed_line.starts_with("//")
        || trimmed_line.starts_with("/*")
        || trimmed_line.starts_with('*')
}

fn check_rust_attr_prefix(prefix: &str) -> bool {
    let t = prefix.trim();
    t.is_empty()
        || t.ends_with(';')
        || t.ends_with('{')
        || t.ends_with('}')
        || prefix.contains("//")
        // Stacked attributes on one line (cfg plus allow).
        || prefix.contains("#[")
}
