use crate::config::InvariantRule;
use crate::engines::util::{is_offset_inside_string, strip_line_comment};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One forbidden import, call, or token crossing an architectural boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantViolation {
    pub file: PathBuf,
    pub line_number: usize,
    pub rule_name: String,
    pub violation_type: String,
    pub offending_target: String,
    pub line_content: String,
    pub message: String,
}

pub struct CompiledInvariantRule {
    pub name: String,
    pub from_glob: GlobSet,
    pub exclude_glob: Option<GlobSet>,
    pub disallow_imports: Option<GlobSet>,
    pub disallow_calls: Option<Vec<Regex>>,
    pub disallow_tokens: Option<Vec<Regex>>,
    pub message: String,
}

pub struct InvariantsChecker {
    rules: Vec<CompiledInvariantRule>,
    import_regexes: Vec<Regex>,
}

struct CheckContext<'a> {
    rel_path: &'a Path,
    line_number: usize,
    line: &'a str,
    rule: &'a CompiledInvariantRule,
}

impl InvariantsChecker {
    pub fn new(rules: &[InvariantRule]) -> Self {
        let compiled_rules = rules.iter().map(compile_rule).collect();
        let import_regexes = vec![
            Regex::new(r#"(?:import|from)\s+['"]([^'"]+)['"]"#).unwrap(),
            Regex::new(r#"(?:require|import)\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap(),
            // Rust: `use crate::db::pool;`, `use self::x::{A, B};`, `use super::y::*;`
            Regex::new(r#"\buse\s+([^;]+);"#).unwrap(),
            Regex::new(r#"(?:from\s+([a-zA-Z0-9_\.]+)\s+import|import\s+([a-zA-Z0-9_\.]+))"#)
                .unwrap(),
        ];

        Self {
            rules: compiled_rules,
            import_regexes,
        }
    }

    pub fn check_file(&self, path: &Path, content: &str, root: &Path) -> Vec<InvariantViolation> {
        let mut violations = Vec::new();
        let rel_path = path.strip_prefix(root).unwrap_or(path);

        for rule in &self.rules {
            if !rule.from_glob.is_match(rel_path) {
                continue;
            }
            if let Some(ref exclude) = rule.exclude_glob
                && exclude.is_match(rel_path)
            {
                continue;
            }

            self.scan_file_lines((rel_path, content), rule, &mut violations);
        }

        violations
    }

    fn scan_file_lines(
        &self,
        file: (&Path, &str),
        rule: &CompiledInvariantRule,
        violations: &mut Vec<InvariantViolation>,
    ) {
        for (idx, line) in file.1.lines().enumerate() {
            let line_number = idx + 1;
            // Strip inline `//` comments (outside strings) so `// use evil`
            // doesn't flag, but real imports still do. String literals are
            // preserved to avoid false positives from `"use evil"` data.
            let code = strip_line_comment(line);
            let trimmed = code.trim();

            if trimmed.is_empty() || is_comment_line(trimmed) {
                continue;
            }

            let ctx = CheckContext {
                rel_path: file.0,
                line_number,
                line: trimmed,
                rule,
            };

            self.check_imports(&ctx, violations);
            check_calls(&ctx, violations);
            check_tokens(&ctx, violations);
        }
    }

    fn check_imports(&self, ctx: &CheckContext, violations: &mut Vec<InvariantViolation>) {
        let Some(ref _imp) = ctx.rule.disallow_imports else {
            return;
        };

        for re in &self.import_regexes {
            for cap in re.captures_iter(ctx.line) {
                self.check_single_import_match(&cap, ctx, violations);
            }
        }
    }

    fn check_single_import_match(
        &self,
        cap: &regex::Captures,
        ctx: &CheckContext,
        violations: &mut Vec<InvariantViolation>,
    ) {
        let Some(ref imp_globs) = ctx.rule.disallow_imports else {
            return;
        };
        let whole = cap.get(0).map(|m| m.start()).unwrap_or(0);
        // Skip keywords inside string literals (data, not imports).
        if is_offset_inside_string(ctx.line, whole) {
            return;
        }
        for i in 1..cap.len() {
            let Some(m) = cap.get(i) else { continue };
            let import_str = m.as_str().trim();
            if import_str.is_empty() {
                continue;
            }
            if check_candidate_blocked(import_str, imp_globs) {
                let candidate = expand_import_candidates(import_str)
                    .into_iter()
                    .find(|c| {
                        imp_globs.is_match(c)
                            || imp_globs.is_match(normalize_rust_import(c).as_str())
                    })
                    .unwrap_or_else(|| import_str.to_string());
                violations.push(create_violation(ctx, "Disallowed Import", &candidate));
                break;
            }
        }
    }
}

fn check_candidate_blocked(import_str: &str, imp_globs: &GlobSet) -> bool {
    expand_import_candidates(import_str)
        .iter()
        .any(|c| imp_globs.is_match(c) || imp_globs.is_match(normalize_rust_import(c).as_str()))
}

fn compile_rule(rule: &InvariantRule) -> CompiledInvariantRule {
    let mut from_builder = GlobSetBuilder::new();
    if let Ok(g) = Glob::new(&rule.from) {
        from_builder.add(g);
    }
    let from_glob = from_builder.build().unwrap_or_else(|_| GlobSet::empty());

    let exclude_glob = rule.exclude.as_ref().map(|ex| build_globset(ex));
    let disallow_imports = rule.disallow_imports.as_ref().map(|im| build_globset(im));

    let disallow_calls = rule.disallow_calls.as_ref().map(|calls| {
        calls
            .iter()
            .filter_map(|c| Regex::new(&format!(r"\b{}\s*\(", regex::escape(c))).ok())
            .collect()
    });

    let disallow_tokens = rule.disallow_tokens.as_ref().map(|tokens| {
        tokens
            .iter()
            .filter_map(|t| Regex::new(&format!(r"\b{}\b", regex::escape(t))).ok())
            .collect()
    });

    CompiledInvariantRule {
        name: rule
            .name
            .clone()
            .unwrap_or_else(|| format!("Rule from {}", rule.from)),
        from_glob,
        exclude_glob,
        disallow_imports,
        disallow_calls,
        disallow_tokens,
        message: rule
            .message
            .clone()
            .unwrap_or_else(|| "Architectural invariant violation".to_string()),
    }
}

fn build_globset(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        if let Ok(g) = Glob::new(p) {
            builder.add(g);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

fn check_calls(ctx: &CheckContext, violations: &mut Vec<InvariantViolation>) {
    let Some(ref call_res) = ctx.rule.disallow_calls else {
        return;
    };
    for re in call_res {
        if let Some(mat) = re.find(ctx.line) {
            let target = mat.as_str().trim_end_matches('(').trim();
            violations.push(create_violation(ctx, "Disallowed Call", target));
        }
    }
}

fn check_tokens(ctx: &CheckContext, violations: &mut Vec<InvariantViolation>) {
    let Some(ref token_res) = ctx.rule.disallow_tokens else {
        return;
    };
    for re in token_res {
        if let Some(mat) = re.find(ctx.line) {
            violations.push(create_violation(ctx, "Disallowed Token", mat.as_str()));
        }
    }
}

fn create_violation(ctx: &CheckContext, vtype: &str, target: &str) -> InvariantViolation {
    InvariantViolation {
        file: ctx.rel_path.to_path_buf(),
        line_number: ctx.line_number,
        rule_name: ctx.rule.name.clone(),
        violation_type: vtype.to_string(),
        offending_target: target.to_string(),
        line_content: ctx.line.to_string(),
        message: ctx.rule.message.clone(),
    }
}

fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*")
}

fn expand_import_candidates(import: &str) -> Vec<String> {
    let base = import.trim().trim_end_matches(',').trim().to_string();
    if base.is_empty() {
        return Vec::new();
    }
    let mut out = vec![base.clone()];
    push_brace_expansions(&base, &mut out);
    push_comma_expansions(&base, &mut out);
    out
}

fn push_brace_expansions(base: &str, out: &mut Vec<String>) {
    let Some((head, tail)) = base.split_once('{') else {
        return;
    };
    for part in tail.trim_end_matches('}').split(',') {
        let p = part
            .trim()
            .trim_matches(|c| c == ' ' || c == '\'' || c == '"');
        if !p.is_empty() {
            out.push(format!("{}{}", head.trim(), p));
        }
    }
}

fn push_comma_expansions(base: &str, out: &mut Vec<String>) {
    if base.contains('{') || !base.contains(',') {
        return;
    }
    for part in base.split(',') {
        let p = part.trim().to_string();
        if !p.is_empty() && p != base {
            out.push(p);
        }
    }
}

fn normalize_rust_import(import: &str) -> String {
    // `crate::db::pool` -> `db/pool`, `super::x` -> `x`, `self::y` -> `y`,
    // `a::B` -> `a/B` so glob `src/db/**` and `*db*` both have a chance.
    let mut s = import.trim().to_string();
    for prefix in ["crate::", "self::", "super::"] {
        while let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
        }
    }
    s = s.replace("::", "/");
    s = s.replace(['{', '}', '*', ' ', '\'', '"'], "");
    s
}
