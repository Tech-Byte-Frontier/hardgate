use crate::config::DeadCodeConfig;
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One unreferenced file or unused export left behind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeViolation {
    pub file: PathBuf,
    pub line_number: Option<usize>,
    pub symbol: Option<String>,
    pub violation_type: String, // "Unreferenced File" or "Unused Export"
    pub message: String,
    pub recommendation: String,
}

pub struct DeadCodeAnalyzer {
    config: DeadCodeConfig,
    entry_globs: GlobSet,
    exclude_globs: GlobSet,
}

struct AnalysisContext<'a> {
    files: &'a [PathBuf],
    file_contents: &'a [(PathBuf, String)],
    root: &'a Path,
}

impl DeadCodeAnalyzer {
    pub fn new(config: &DeadCodeConfig) -> Self {
        let entry_globs = build_entry_globs(&config.entry_points);
        let exclude_globs = build_exclude_globs(&config.exclude);

        Self {
            config: config.clone(),
            entry_globs,
            exclude_globs,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn analyze(
        &self,
        files: &[PathBuf],
        file_contents: &[(PathBuf, String)],
        root: &Path,
    ) -> Vec<DeadCodeViolation> {
        let mut violations = Vec::new();
        let ctx = AnalysisContext {
            files,
            file_contents,
            root,
        };

        let referenced_stems = collect_referenced_stems(file_contents);
        self.detect_unreferenced_files(&ctx, &referenced_stems, &mut violations);
        self.detect_unused_exports(&ctx, &mut violations);

        violations
    }

    fn detect_unreferenced_files(
        &self,
        ctx: &AnalysisContext,
        referenced_stems: &HashSet<String>,
        violations: &mut Vec<DeadCodeViolation>,
    ) {
        for path in ctx.files {
            let rel = path.strip_prefix(ctx.root).unwrap_or(path);

            if self.is_ignored(rel) {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };

            if is_index_or_entry_stem(stem) || referenced_stems.contains(stem) {
                continue;
            }

            violations.push(DeadCodeViolation {
                file: rel.to_path_buf(),
                line_number: Some(1),
                symbol: None,
                violation_type: "Unreferenced File".to_string(),
                message: format!(
                    "File `{}` is never imported or referenced in this project.",
                    rel.display()
                ),
                recommendation: "Remove this file or import it from an active module.".to_string(),
            });
        }
    }

    fn detect_unused_exports(
        &self,
        ctx: &AnalysisContext,
        violations: &mut Vec<DeadCodeViolation>,
    ) {
        for (path, content) in ctx.file_contents {
            let rel = path.strip_prefix(ctx.root).unwrap_or(path);
            if self.is_ignored(rel) || !is_js_or_ts_file(path) {
                continue;
            }

            let declared_exports = find_declared_exports(content);
            for (line_num, symbol) in declared_exports {
                if !is_symbol_referenced(&symbol, path, ctx.file_contents) {
                    violations.push(DeadCodeViolation {
                        file: rel.to_path_buf(),
                        line_number: Some(line_num),
                        symbol: Some(symbol.clone()),
                        violation_type: "Unused Export".to_string(),
                        message: format!(
                            "Exported symbol `{}` in `{}:{}` is never referenced across the project.",
                            symbol, rel.display(), line_num
                        ),
                        recommendation: format!("Remove unused export `{}` or unexport if only used locally.", symbol),
                    });
                }
            }
        }
    }

    fn is_ignored(&self, rel: &Path) -> bool {
        self.entry_globs.is_match(rel) || self.exclude_globs.is_match(rel)
    }
}

fn build_entry_globs(user_entries: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    let default_entries = [
        "src/main.rs",
        "src/lib.rs",
        "src/index.ts",
        "src/index.tsx",
        "src/main.ts",
        "src/main.tsx",
        "src/App.tsx",
        "src/app.tsx",
        "main.rs",
        "lib.rs",
        "index.ts",
        "index.tsx",
        "**/*.d.ts",
        "**/build.rs",
        "**/vite.config.*",
        "**/next.config.*",
        "**/tailwind.config.*",
    ];
    for entry in &default_entries {
        if let Ok(g) = Glob::new(entry) {
            builder.add(g);
        }
    }
    for entry in user_entries {
        if let Ok(g) = Glob::new(entry) {
            builder.add(g);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

fn build_exclude_globs(user_excludes: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    let default_excludes = [
        "tests/**",
        "**/*_test.rs",
        "**/*.test.ts",
        "**/*.test.tsx",
        "**/*.test.js",
        "**/*.test.jsx",
        "**/*.spec.ts",
        "**/*.spec.tsx",
        "**/*.spec.js",
        "**/__tests__/**",
    ];
    for ex in &default_excludes {
        if let Ok(g) = Glob::new(ex) {
            builder.add(g);
        }
    }
    for ex in user_excludes {
        if let Ok(g) = Glob::new(ex) {
            builder.add(g);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

fn import_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?:import|from|require)\s*\(?['"]([^'"]+)['"]"#).expect("valid import regex")
    })
}

fn rust_mod_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\bmod\s+([a-zA-Z0-9_]+);"#).expect("valid mod regex"))
}

fn rust_path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"#\[path\s*=\s*["']([^"']+)["']\]\s*(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+[a-zA-Z0-9_]+\s*;"#,
        )
        .expect("valid path module regex")
    })
}

fn rust_use_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"\buse\s+(?:crate::|super::)?([a-zA-Z0-9_]+)"#).expect("valid use regex")
    })
}

fn export_fn_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"export\s+(?:async\s+)?function\s+([a-zA-Z0-9_]+)"#)
            .expect("valid export fn regex")
    })
}

fn export_const_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"export\s+const\s+([a-zA-Z0-9_]+)"#).expect("valid export const regex")
    })
}

fn collect_referenced_stems(file_contents: &[(PathBuf, String)]) -> HashSet<String> {
    let mut stems = HashSet::new();
    let import_re = import_regex();
    let rust_mod_re = rust_mod_regex();
    let rust_path_re = rust_path_regex();
    let rust_use_re = rust_use_regex();

    for (_, content) in file_contents {
        scan_import_stems(content, import_re, &mut stems);
        scan_rust_stems(content, rust_mod_re, &mut stems);
        scan_rust_path_stems(content, rust_path_re, &mut stems);
        scan_rust_stems(content, rust_use_re, &mut stems);
    }
    stems
}

fn scan_import_stems(content: &str, re: &Regex, stems: &mut HashSet<String>) {
    for cap in re.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            let imp = m.as_str();
            let file_name = Path::new(imp)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(imp);
            let stem = Path::new(file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(file_name);
            stems.insert(stem.to_string());
        }
    }
}

fn scan_rust_stems(content: &str, re: &Regex, stems: &mut HashSet<String>) {
    for cap in re.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            stems.insert(m.as_str().to_string());
        }
    }
}

fn scan_rust_path_stems(content: &str, re: &Regex, stems: &mut HashSet<String>) {
    for cap in re.captures_iter(content) {
        let Some(path) = cap.get(1).map(|m| Path::new(m.as_str())) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        stems.insert(stem.to_string());
    }
}

fn is_index_or_entry_stem(stem: &str) -> bool {
    matches!(stem, "mod" | "index" | "lib" | "main" | "App" | "app")
}

fn is_js_or_ts_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext,
        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs"
    )
}

fn find_declared_exports(content: &str) -> Vec<(usize, String)> {
    let export_fn_re = export_fn_regex();
    let export_const_re = export_const_regex();
    let mut exports = Vec::new();

    for (idx, line) in content.lines().enumerate() {
        let line_num = idx + 1;
        if let Some(cap) = export_fn_re.captures(line) {
            if let Some(sym) = cap.get(1) {
                exports.push((line_num, sym.as_str().to_string()));
            }
        } else if let Some(cap) = export_const_re.captures(line)
            && let Some(sym) = cap.get(1)
        {
            exports.push((line_num, sym.as_str().to_string()));
        }
    }
    exports
}

fn is_symbol_referenced(
    symbol: &str,
    current_file: &Path,
    file_contents: &[(PathBuf, String)],
) -> bool {
    if symbol == "default" || symbol.starts_with('_') {
        return true;
    }
    // Word-boundary search: `used` must not match `unusedFunc`.
    let pattern = format!(r"\b{}\b", regex::escape(symbol));
    let Ok(re) = Regex::new(&pattern) else {
        return true;
    };
    for (other_path, other_content) in file_contents {
        if other_path != current_file && re.is_match(other_content) {
            return true;
        }
    }
    false
}
