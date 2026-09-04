use super::js_command::{JsCommandInput, build_js_command};
use super::js_tests::find_relevant_test;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Package managers understood by the native JavaScript test resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }
}

/// Test frameworks that have a stable command-line file selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestFramework {
    Jest,
    Vitest,
    Playwright,
}

impl TestFramework {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jest => "jest",
            Self::Vitest => "vitest",
            Self::Playwright => "playwright",
        }
    }

    pub(crate) fn binary(self) -> &'static str {
        match self {
            Self::Jest => "jest",
            Self::Vitest => "vitest",
            Self::Playwright => "playwright",
        }
    }

    pub(crate) fn args(self) -> &'static str {
        match self {
            Self::Jest => "",
            Self::Vitest => "run",
            Self::Playwright => "test",
        }
    }
}

/// How the automatically selected test command scopes its execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestSelection {
    Relevant(PathBuf),
    FullSuite,
    Custom,
}

impl TestSelection {
    pub fn is_full_suite(&self) -> bool {
        matches!(self, Self::FullSuite)
    }

    pub fn relevant_test(&self) -> Option<&Path> {
        match self {
            Self::Relevant(path) => Some(path),
            Self::FullSuite | Self::Custom => None,
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Relevant(_) => "relevant test",
            Self::FullSuite => "full suite (no reliable test match)",
            Self::Custom => "custom command",
        }
    }
}

/// Fully resolved command and working directory for one mutation baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTestPlan {
    pub command: String,
    pub working_dir: PathBuf,
    pub package_root: PathBuf,
    pub workspace_root: PathBuf,
    pub manager: Option<PackageManager>,
    pub framework: Option<TestFramework>,
    pub selection: TestSelection,
    pub recommended_timeout_secs: u64,
}

impl ResolvedTestPlan {
    pub fn full_suite_timeout_required(&self) -> bool {
        self.selection.is_full_suite() && self.recommended_timeout_secs > 0
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PackageMetadata {
    pub(crate) root: PathBuf,
    scripts: BTreeMap<String, String>,
    package_manager: Option<PackageManager>,
    framework: Option<TestFramework>,
    workspaces: bool,
}

#[derive(Debug, Clone)]
struct FrameworkConfig {
    framework: TestFramework,
    root: PathBuf,
}

pub(crate) fn resolve_js_test_plan(file: &Path, root: &Path) -> ResolvedTestPlan {
    let source = absolute_source_path(file, root);
    let dirs = ancestor_dirs(source.parent().unwrap_or(&source), root);
    let package = find_package(&dirs);
    let workspace_root = find_workspace_root(&dirs, package.as_ref(), root);
    let manager = detect_manager(&dirs, package.as_ref());
    let config = package
        .as_ref()
        .and_then(|item| {
            item.framework.map(|framework| FrameworkConfig {
                framework,
                root: item.root.clone(),
            })
        })
        .or_else(|| find_framework_config(&dirs));
    let script = package.as_ref().and_then(test_script);
    let script_framework = script
        .as_ref()
        .and_then(|(_, command)| framework_from_text(command));
    let framework = script_framework.or_else(|| config.as_ref().map(|item| item.framework));
    let execution_root =
        select_execution_root(package.as_ref(), config.as_ref(), script.is_some(), root);
    let candidate = find_relevant_test(&source, &execution_root, package.as_ref());
    let selection = candidate
        .clone()
        .map(TestSelection::Relevant)
        .unwrap_or(TestSelection::FullSuite);
    let command = build_js_command(JsCommandInput {
        manager,
        framework,
        script: script.as_ref().map(|(name, _)| name.as_str()),
        candidate: selection.relevant_test(),
        working_dir: &execution_root,
    });

    ResolvedTestPlan {
        command,
        working_dir: execution_root,
        package_root: package
            .as_ref()
            .map(|item| item.root.clone())
            .unwrap_or_else(|| root.to_path_buf()),
        workspace_root,
        manager: Some(manager),
        framework,
        recommended_timeout_secs: if selection.is_full_suite() { 60 } else { 10 },
        selection,
    }
}

fn absolute_source_path(file: &Path, root: &Path) -> PathBuf {
    let path = if file.is_absolute() {
        file.to_path_buf()
    } else {
        root.join(file)
    };
    fs::canonicalize(&path).unwrap_or(path)
}

fn ancestor_dirs(start: &Path, root: &Path) -> Vec<PathBuf> {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut dirs = Vec::new();
    let mut current = fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    loop {
        dirs.push(current.clone());
        if current == root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current || !parent.starts_with(&root) {
            if !dirs.iter().any(|item| item == &root) {
                dirs.push(root.clone());
            }
            break;
        }
        current = parent.to_path_buf();
    }
    dirs
}

fn find_package(dirs: &[PathBuf]) -> Option<PackageMetadata> {
    dirs.iter().find_map(|dir| {
        let manifest = dir.join("package.json");
        if !manifest.is_file() {
            return None;
        }
        Some(read_package(&manifest, dir))
    })
}

fn read_package(manifest: &Path, root: &Path) -> PackageMetadata {
    let value = fs::read_to_string(manifest)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .unwrap_or(Value::Null);
    let scripts = value
        .get("scripts")
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let package_manager = value
        .get("packageManager")
        .and_then(Value::as_str)
        .and_then(parse_package_manager);
    let framework = [
        ("jest", TestFramework::Jest),
        ("vitest", TestFramework::Vitest),
        ("playwright", TestFramework::Playwright),
    ]
    .iter()
    .find_map(|(key, framework)| value.get(*key).is_some().then_some(*framework));
    let workspaces = value.get("workspaces").is_some();
    PackageMetadata {
        root: root.to_path_buf(),
        scripts,
        package_manager,
        framework,
        workspaces,
    }
}

fn find_workspace_root(
    dirs: &[PathBuf],
    package: Option<&PackageMetadata>,
    fallback: &Path,
) -> PathBuf {
    dirs.iter()
        .find(|dir| is_workspace_marker(dir))
        .cloned()
        .or_else(|| package.map(|item| item.root.clone()))
        .unwrap_or_else(|| fallback.to_path_buf())
}

fn is_workspace_marker(dir: &Path) -> bool {
    [
        "pnpm-workspace.yaml",
        "pnpm-lock.yaml",
        "yarn.lock",
        ".yarnrc.yml",
        "bun.lock",
        "bun.lockb",
        "bunfig.toml",
        "package-lock.json",
        "npm-shrinkwrap.json",
    ]
    .iter()
    .any(|name| dir.join(name).is_file())
        || read_package(&dir.join("package.json"), dir).workspaces
}

fn detect_manager(dirs: &[PathBuf], package: Option<&PackageMetadata>) -> PackageManager {
    if let Some(manager) = package.and_then(|item| item.package_manager) {
        return manager;
    }
    for dir in dirs {
        if let Some(manager) = package_manager_in(dir) {
            return manager;
        }
    }
    PackageManager::Npm
}

fn package_manager_in(dir: &Path) -> Option<PackageManager> {
    let package_manager = dir
        .join("package.json")
        .is_file()
        .then(|| read_package(&dir.join("package.json"), dir).package_manager)
        .flatten();
    package_manager.or_else(|| {
        [
            ("pnpm-lock.yaml", PackageManager::Pnpm),
            ("pnpm-workspace.yaml", PackageManager::Pnpm),
            ("yarn.lock", PackageManager::Yarn),
            (".yarnrc.yml", PackageManager::Yarn),
            ("bun.lock", PackageManager::Bun),
            ("bun.lockb", PackageManager::Bun),
            ("bunfig.toml", PackageManager::Bun),
            ("package-lock.json", PackageManager::Npm),
            ("npm-shrinkwrap.json", PackageManager::Npm),
        ]
        .iter()
        .find_map(|(name, manager)| dir.join(name).is_file().then_some(*manager))
    })
}

fn find_framework_config(dirs: &[PathBuf]) -> Option<FrameworkConfig> {
    dirs.iter().find_map(|dir| {
        [
            ("jest", TestFramework::Jest),
            ("vitest", TestFramework::Vitest),
            ("playwright", TestFramework::Playwright),
        ]
        .iter()
        .find_map(|(prefix, framework)| {
            config_exists(dir, prefix).then_some(FrameworkConfig {
                framework: *framework,
                root: dir.clone(),
            })
        })
    })
}

fn config_exists(dir: &Path, prefix: &str) -> bool {
    ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "json"]
        .iter()
        .map(|ext| dir.join(format!("{prefix}.config.{ext}")))
        .any(|path| path.is_file())
}

fn test_script(package: &PackageMetadata) -> Option<(String, String)> {
    package
        .scripts
        .get("test")
        .map(|command| ("test".to_string(), command.clone()))
        .or_else(|| {
            package
                .scripts
                .iter()
                .find(|(name, _)| name.starts_with("test:"))
                .map(|(name, command)| (name.clone(), command.clone()))
        })
}

fn framework_from_text(command: &str) -> Option<TestFramework> {
    let lower = command.to_ascii_lowercase();
    [
        ("playwright", TestFramework::Playwright),
        ("vitest", TestFramework::Vitest),
        ("jest", TestFramework::Jest),
    ]
    .iter()
    .find_map(|(needle, framework)| lower.contains(needle).then_some(*framework))
}

fn select_execution_root(
    package: Option<&PackageMetadata>,
    config: Option<&FrameworkConfig>,
    has_script: bool,
    fallback: &Path,
) -> PathBuf {
    if has_script {
        return package
            .map(|item| item.root.clone())
            .or_else(|| config.map(|item| item.root.clone()))
            .unwrap_or_else(|| fallback.to_path_buf());
    }
    config
        .map(|item| item.root.clone())
        .or_else(|| package.map(|item| item.root.clone()))
        .unwrap_or_else(|| fallback.to_path_buf())
}

fn parse_package_manager(value: &str) -> Option<PackageManager> {
    let name = value.split('@').next()?.trim().to_ascii_lowercase();
    match name.as_str() {
        "npm" => Some(PackageManager::Npm),
        "pnpm" => Some(PackageManager::Pnpm),
        "yarn" => Some(PackageManager::Yarn),
        "bun" => Some(PackageManager::Bun),
        _ => None,
    }
}

pub(crate) fn is_javascript_path(file: &Path) -> bool {
    file.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts"
            )
        })
}
