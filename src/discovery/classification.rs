use anyhow::{Context, Result};
use globset::Glob;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

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
    if contains_component(path, VENDOR_DIRS) || contains_component_str(&normalized, VENDOR_DIRS) {
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
            let reason = rule
                .reason
                .clone()
                .unwrap_or_else(|| format!("custom classification rule {index}: {}", rule.glob));
            return Ok((rule.role, reason));
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
    if contains_component_str(path, VENDOR_DIRS) {
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
    if is_configuration(file_name) {
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
    contains_any(
        path,
        &[
            "__generated__/",
            "generated/",
            "gen/",
            "/__generated__/",
            "/generated/",
            "/gen/",
        ],
    ) || contains_any(file_name, &[".generated.", ".gen."])
        || matches!(file_name, "database.types.ts" | "schema.gen.ts")
}

fn is_fixture(path: &str, file_name: &str) -> bool {
    contains_any(
        path,
        &[
            "__fixtures__/",
            "fixtures/",
            "__snapshots__/",
            "snapshots/",
            "/__fixtures__/",
            "/fixtures/",
            "/__snapshots__/",
            "/snapshots/",
        ],
    ) || contains_any(file_name, &[".fixture.", ".fixtures.", ".snap"])
}

fn is_test(path: &str, file_name: &str) -> bool {
    contains_any(
        path,
        &[
            "tests/",
            "test/",
            "/tests/",
            "/test/",
            "__tests__/",
            "/__tests__/",
            "__mocks__/",
            "/__mocks__/",
            "mocks/",
            "/mocks/",
            "stories/",
            "/stories/",
        ],
    ) || contains_any(file_name, &[".test.", ".spec.", ".stories.", ".mock."])
}

fn is_migration(path: &str, file_name: &str) -> bool {
    path.contains("/migrations/")
        || path.starts_with("migrations/")
        || path.contains("/migration/")
        || path.starts_with("migration/")
        || file_name == "seed.sql"
        || file_name == "seed.ts"
        || file_name.ends_with(".migration.sql")
        || file_name.ends_with(".seed.sql")
}

fn is_configuration(file_name: &str) -> bool {
    matches!(
        extension(file_name),
        "json" | "jsonc" | "toml" | "yaml" | "yml"
    )
}

fn is_documentation(file_name: &str) -> bool {
    matches!(extension(file_name), "mdx")
}

fn extension(file_name: &str) -> &str {
    file_name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}

fn contains_component(path: &Path, candidates: &[&str]) -> bool {
    path.components().any(|component| match component {
        Component::Normal(value) => value.to_str().is_some_and(|part| {
            candidates
                .iter()
                .any(|candidate| part.eq_ignore_ascii_case(candidate))
        }),
        _ => false,
    })
}

fn contains_component_str(path: &str, candidates: &[&str]) -> bool {
    path.split('/').any(|part| candidates.contains(&part))
}

fn contains_any(value: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| value.contains(marker))
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
