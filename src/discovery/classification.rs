use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

/// Repository role assigned before any analysis engine chooses its inputs.
///
/// Roles are deliberately independent from technical-debt exclusions: a file
/// remains classified even when one engine has an explicit exclusion for it.
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
    pub fn new(path: &Path) -> Self {
        let (role, reason) = classify_role(path);
        Self {
            path: path.to_path_buf(),
            role,
            ast_supported: ast_supported(path),
            reason: reason.to_string(),
        }
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
        .is_some_and(|ext| INVENTORY_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

pub fn ast_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| AST_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn classify_role(path: &Path) -> (FileRole, &'static str) {
    let normalized = normalize(path);
    let file_name = normalized.rsplit('/').next().unwrap_or_default();

    if contains_component(path, VENDOR_DIRS) {
        return (FileRole::Vendor, "dependency or build-output directory");
    }
    if is_generated(&normalized, file_name) {
        return (FileRole::Generated, "generated-code convention");
    }
    if is_fixture(&normalized, file_name) {
        return (FileRole::Fixture, "fixture or snapshot convention");
    }
    if is_test(&normalized, file_name) {
        return (FileRole::Test, "test, mock, or story convention");
    }
    if is_migration(&normalized, file_name) {
        return (FileRole::Migration, "migration or seed convention");
    }
    if is_configuration(file_name) {
        return (FileRole::Config, "configuration/data extension");
    }
    if is_documentation(file_name) {
        return (FileRole::Documentation, "documentation extension");
    }
    if is_inventory_file(path) {
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
    path.contains("/__generated__/")
        || path.contains("/generated/")
        || file_name.contains(".generated.")
        || file_name.contains(".gen.")
        || matches!(file_name, "database.types.ts" | "schema.gen.ts")
}

fn is_fixture(path: &str, file_name: &str) -> bool {
    path.contains("/__fixtures__/")
        || path.contains("/fixtures/")
        || file_name.contains(".fixture.")
        || file_name.ends_with(".snap")
}

fn is_test(path: &str, file_name: &str) -> bool {
    path.starts_with("tests/")
        || path.contains("/tests/")
        || path.contains("/__tests__/")
        || path.contains("/__mocks__/")
        || path.contains("/mocks/")
        || [".test.", ".spec.", ".stories.", ".mock."]
            .iter()
            .any(|marker| file_name.contains(marker))
}

fn is_migration(path: &str, file_name: &str) -> bool {
    path.contains("/migrations/")
        || path.starts_with("migrations/")
        || (path.contains("supabase/") && matches!(file_name, "seed.sql" | "seed.ts"))
        || file_name.ends_with(".migration.sql")
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
        Component::Normal(value) => value
            .to_str()
            .is_some_and(|part| candidates.contains(&part)),
        _ => false,
    })
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_ascii_lowercase()
}
