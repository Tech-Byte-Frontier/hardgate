use anyhow::{Context, Result};
use globset::Glob;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::config::ClassificationConfig;

/// Repository role assigned before any analysis engine chooses its inputs.
///
/// Roles are deliberately independent from technical-debt exclusions: a file
/// remains classified even when one engine has an explicit exclusion for it.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileRole {
    Source,
    Test,
    Generated,
    Fixture,
    Vendor,
    Migration,
    Config,
    Documentation,
    Unknown,
}

impl FileRole {
    /// Production code is the only default target for native mutation.
    pub fn is_mutation_target(self) -> bool {
        self == Self::Source
    }

    /// Handwritten code receives AST complexity analysis when a parser exists.
    pub fn receives_complexity(self) -> bool {
        matches!(self, Self::Source | Self::Test)
    }

    /// Files that receive ordinary clone analysis by default.
    pub fn receives_clone_analysis(self) -> bool {
        matches!(self, Self::Source | Self::Test | Self::Fixture)
    }

    /// Files that receive size and anti-suppression safety checks by default.
    pub fn receives_safety_checks(self) -> bool {
        matches!(
            self,
            Self::Source | Self::Test | Self::Fixture | Self::Migration | Self::Config
        )
    }

    /// Every role represented by a first-class role policy section.
    pub const POLICY_ROLES: [Self; 5] = [
        Self::Source,
        Self::Test,
        Self::Generated,
        Self::Fixture,
        Self::Migration,
    ];
}

/// One inventory entry, including whether Hardgate has an AST parser for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedFile {
    pub path: std::path::PathBuf,
    pub role: FileRole,
    pub ast_supported: bool,
    pub reason: String,
}

impl ClassifiedFile {
    /// Classify using the built-in conventions retained for backwards
    /// compatibility.
    pub fn new(path: &Path) -> Self {
        let (role, reason) = classify_builtin(path);
        Self {
            path: path.to_path_buf(),
            role,
            ast_supported: ast_supported(path),
            reason: reason.to_string(),
        }
    }

    /// Classify using ordered custom rules followed by built-ins.
    ///
    /// Vendor/build directories remain authoritative and cannot be re-enabled
    /// by a custom rule.  Invalid rules are returned instead of silently
    /// falling back to built-ins; configuration loading validates them before
    /// engines run, while this method keeps direct API use fail-closed too.
    pub fn new_with_config(path: &Path, config: &ClassificationConfig) -> Result<Self> {
        let (role, reason) = classify_with_config(path, config)?;
        Ok(Self {
            path: path.to_path_buf(),
            role,
            ast_supported: ast_supported(path),
            reason,
        })
    }
}

/// Extensions with a Tree-sitter parser in Hardgate.
pub const AST_EXTENSIONS: &[&str] = &[
    "rs", "js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "py", "go",
];

/// Text formats intentionally inventoried even when no AST engine supports
/// them. This prevents Markdown/SQL/data files from disappearing silently.
pub const INVENTORY_EXTENSIONS: &[&str] = &[
    "rs", "js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "py", "go", "css", "mdx", "sql",
    "json", "jsonc", "graphql", "gql", "snap", "toml", "yaml", "yml",
];

pub fn is_inventory_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|ext| INVENTORY_EXTENSIONS.contains(&ext.as_str()))
}

pub fn ast_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|ext| AST_EXTENSIONS.contains(&ext.as_str()))
}

/// Classify a path with custom ordered rules. The first matching rule wins.
pub fn classify_with_config(
    path: &Path,
    config: &ClassificationConfig,
) -> Result<(FileRole, String)> {
    let normalized = normalize(path);
    let file_name = normalized.rsplit('/').next().unwrap_or_default();

    // Discovery prunes these directories, and explicit scans must preserve the
    // same safety boundary even when a user rule tries to override it.
    if has_directory_component(&normalized, VENDOR_DIRS) {
        return Ok((
            FileRole::Vendor,
            "dependency or build-output directory".to_string(),
        ));
    }

    let candidates = path_candidates(&normalized);
    for (index, rule) in config.rules.iter().enumerate() {
        let pattern = rule.glob.trim().replace('\\', "/").to_ascii_lowercase();
        let matcher = Glob::new(&pattern)
            .with_context(|| format!("Invalid classification glob `{}`", rule.glob))?
            .compile_matcher();
        if candidates
            .iter()
            .any(|candidate| matcher.is_match(candidate))
        {
            return Ok((
                rule.role,
                format!("custom classification rule {index}: {}", rule.glob),
            ));
        }
    }

    let (role, reason) = classify_builtin_parts(&normalized, file_name);
    Ok((role, reason.to_string()))
}

fn classify_builtin(path: &Path) -> (FileRole, &'static str) {
    let normalized = normalize(path);
    let file_name = normalized.rsplit('/').next().unwrap_or_default();
    classify_builtin_parts(&normalized, file_name)
}

fn classify_builtin_parts(path: &str, file_name: &str) -> (FileRole, &'static str) {
    if has_directory_component(path, VENDOR_DIRS) {
        return (FileRole::Vendor, "dependency or build-output directory");
    }
    if is_generated(path, file_name) {
        return (FileRole::Generated, "generated-code convention");
    }
    if is_fixture(path, file_name) {
        return (FileRole::Fixture, "fixture or snapshot convention");
    }
    if is_test(path, file_name) {
        return (FileRole::Test, "test, mock, or story convention");
    }
    if is_migration(path, file_name) {
        return (FileRole::Migration, "migration or seed convention");
    }
    if is_configuration(path, file_name) {
        return (FileRole::Config, "configuration/data extension");
    }
    if is_documentation(file_name) {
        return (FileRole::Documentation, "documentation extension");
    }
    if is_inventory_file(Path::new(path)) {
        return (FileRole::Source, "handwritten source extension");
    }
    (FileRole::Unknown, "no built-in classification rule")
}

const VENDOR_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    ".venv",
    "venv",
    "__pycache__",
];

fn is_generated(path: &str, file_name: &str) -> bool {
    has_directory_component(path, &["__generated__", "generated", "gen"])
        || has_filename_token(file_name, "generated")
        || has_filename_token(file_name, "gen")
        || matches!(file_name, "database.types.ts" | "schema.gen.ts")
}

fn is_fixture(path: &str, file_name: &str) -> bool {
    has_directory_component(
        path,
        &["__fixtures__", "fixtures", "__snapshots__", "snapshots"],
    ) || has_filename_token(file_name, "fixture")
        || has_filename_token(file_name, "fixtures")
        || extension(file_name) == "snap"
}

fn is_test(path: &str, file_name: &str) -> bool {
    has_directory_component(
        path,
        &[
            "tests",
            "test",
            "__tests__",
            "__mocks__",
            "mocks",
            "stories",
        ],
    ) || has_filename_token(file_name, "test")
        || has_filename_token(file_name, "spec")
        || has_filename_token(file_name, "stories")
        || has_filename_token(file_name, "mock")
}

fn is_migration(path: &str, file_name: &str) -> bool {
    has_directory_component(path, &["migrations", "migration"])
        || file_name == "seed.sql"
        || file_name == "seed.ts"
        || file_name.ends_with(".migration.sql")
        || file_name.ends_with(".seed.sql")
}

fn is_configuration(path: &str, file_name: &str) -> bool {
    has_directory_component(path, &["config", "configs"])
        || has_filename_token(file_name, "config")
        || is_rc_configuration(file_name)
        || matches!(
            extension(file_name),
            "json" | "jsonc" | "toml" | "yaml" | "yml"
        )
}

fn is_rc_configuration(file_name: &str) -> bool {
    let Some(name) = file_name.strip_prefix('.') else {
        return false;
    };
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    stem.len() > 2 && stem.ends_with("rc") && INVENTORY_EXTENSIONS.contains(&extension)
}

fn is_documentation(file_name: &str) -> bool {
    matches!(extension(file_name), "mdx")
}

fn extension(file_name: &str) -> &str {
    file_name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}

fn has_directory_component(path: &str, candidates: &[&str]) -> bool {
    let mut components = path.split('/');
    while let Some(component) = components.next() {
        if candidates.contains(&component) && components.clone().next().is_some() {
            return true;
        }
    }
    false
}

/// Match a marker bounded by dots in a filename (for example, `button.test.ts`).
/// Requiring both delimiters keeps names such as `latest.ts` and
/// `storybooked.ts` out of the test conventions.
fn has_filename_token(file_name: &str, token: &str) -> bool {
    file_name
        .split('.')
        .collect::<Vec<_>>()
        .windows(3)
        .any(|window| window[1] == token)
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_ascii_lowercase()
}

fn path_candidates(normalized: &str) -> Vec<String> {
    let mut candidates = vec![normalized.to_string()];
    for (index, byte) in normalized.bytes().enumerate() {
        if byte == b'/' && index + 1 < normalized.len() {
            candidates.push(normalized[index + 1..].to_string());
        }
    }
    candidates
}
