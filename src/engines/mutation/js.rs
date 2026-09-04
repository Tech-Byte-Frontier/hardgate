use super::js_command::{
    JsCommandInput, build_js_command, framework_from_command, is_exact_bun_test_command,
};
use super::js_tests::{find_relevant_test, parse_workspaces, valid_pnpm_workspace_file};
use anyhow::{Context, Result};
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
    framework_hint_ambiguous: bool,
    workspaces: bool,
}
#[derive(Debug, Clone)]
struct FrameworkConfig {
    framework: TestFramework,
    root: PathBuf,
}
#[derive(Debug, Clone, Default)]
struct FrameworkConfigSearch {
    selected: Option<FrameworkConfig>,
    ambiguous: bool,
}
/// Resolve one JavaScript/TypeScript source to a package-local test command.
/// Existing but unreadable or malformed manifests return an error instead of
/// falling back to an ancestor; callers must report it before invocation.
pub(crate) fn resolve_js_test_plan(file: &Path, root: &Path) -> Result<ResolvedTestPlan> {
    let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let source = absolute_source_path(file, &canonical_root);
    let dirs = ancestor_dirs(source.parent().unwrap_or(&source), &canonical_root);
    let packages = load_packages(&dirs)?;
    let package = packages.first();
    let workspace_root = find_workspace_root(&dirs, &packages, &canonical_root);
    let manager = detect_manager(&dirs, &packages);
    let config = find_framework_config(&dirs);
    let script = package.and_then(test_script);
    let script_framework = script
        .as_ref()
        .and_then(|(_, command)| framework_from_command(command));
    let framework = if script.is_some() {
        script_framework
    } else {
        framework_without_script(package, &config)
    };
    let execution_root = select_execution_root(
        package,
        config.selected.as_ref(),
        script.is_some(),
        &canonical_root,
    );
    let bun_test_script = manager == PackageManager::Bun
        && script
            .as_ref()
            .is_some_and(|(_, command)| is_exact_bun_test_command(command));
    let selector_capable = framework.is_some() || bun_test_script;
    let candidate = selector_capable
        .then(|| find_relevant_test(&source, &execution_root, package))
        .flatten();
    let selection = candidate
        .map(TestSelection::Relevant)
        .unwrap_or(TestSelection::FullSuite);
    let command = build_js_command(JsCommandInput {
        manager,
        framework,
        script: script.as_ref().map(|(name, _)| name.as_str()),
        candidate: selection.relevant_test(),
        selector_capable,
        bun_test_script,
        working_dir: &execution_root,
    });

    Ok(ResolvedTestPlan {
        command,
        working_dir: execution_root,
        package_root: package
            .map(|item| item.root.clone())
            .unwrap_or(canonical_root),
        workspace_root,
        manager: Some(manager),
        framework,
        recommended_timeout_secs: if selection.is_full_suite() { 60 } else { 10 },
        selection,
    })
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

fn load_packages(dirs: &[PathBuf]) -> Result<Vec<PackageMetadata>> {
    let mut packages = Vec::new();
    for dir in dirs {
        let manifest = dir.join("package.json");
        match fs::symlink_metadata(&manifest) {
            Ok(_) => packages.push(read_package(&manifest, dir)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect JavaScript package manifest `{}`",
                        manifest.display()
                    )
                });
            }
        }
    }
    Ok(packages)
}

fn read_package(manifest: &Path, root: &Path) -> Result<PackageMetadata> {
    let content = fs::read_to_string(manifest).with_context(|| {
        format!(
            "failed to read JavaScript package manifest `{}`",
            manifest.display()
        )
    })?;
    let value = serde_json::from_str::<Value>(&content).with_context(|| {
        format!(
            "malformed JavaScript package manifest `{}`",
            manifest.display()
        )
    })?;
    let object = value.as_object().with_context(|| {
        format!(
            "JavaScript package manifest `{}` must contain a JSON object",
            manifest.display()
        )
    })?;
    let scripts = parse_scripts(object, manifest)?;
    let package_manager = parse_manifest_manager(object, manifest)?;
    let (framework, framework_hint_ambiguous) = parse_framework_hints(object);
    Ok(PackageMetadata {
        root: root.to_path_buf(),
        scripts,
        package_manager,
        framework,
        framework_hint_ambiguous,
        workspaces: parse_workspaces(object.get("workspaces")),
    })
}

fn parse_scripts(
    object: &serde_json::Map<String, Value>,
    manifest: &Path,
) -> Result<BTreeMap<String, String>> {
    let Some(value) = object.get("scripts") else {
        return Ok(BTreeMap::new());
    };
    let scripts = value.as_object().with_context(|| {
        format!(
            "JavaScript package manifest `{}` has a non-object `scripts` field",
            manifest.display()
        )
    })?;
    scripts
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|command| (key.clone(), command.to_string()))
                .with_context(|| {
                    format!(
                        "JavaScript package manifest `{}` has a non-string script `{key}`",
                        manifest.display()
                    )
                })
        })
        .collect()
}

fn parse_manifest_manager(
    object: &serde_json::Map<String, Value>,
    manifest: &Path,
) -> Result<Option<PackageManager>> {
    let Some(value) = object.get("packageManager") else {
        return Ok(None);
    };
    let value = value.as_str().with_context(|| {
        format!(
            "JavaScript package manifest `{}` has a non-string `packageManager` field",
            manifest.display()
        )
    })?;
    let manager = parse_package_manager(value).with_context(|| {
        format!(
            "JavaScript package manifest `{}` has unsupported package manager `{value}`",
            manifest.display()
        )
    })?;
    Ok(Some(manager))
}

fn parse_framework_hints(object: &serde_json::Map<String, Value>) -> (Option<TestFramework>, bool) {
    let mut hints = Vec::new();
    let mut malformed = false;
    for (key, framework) in [
        ("jest", TestFramework::Jest),
        ("vitest", TestFramework::Vitest),
        ("playwright", TestFramework::Playwright),
    ] {
        let Some(value) = object.get(key) else {
            continue;
        };
        if value.is_object() {
            hints.push(framework);
        } else {
            malformed = true;
        }
    }
    let ambiguous = malformed || hints.len() > 1;
    (hints.into_iter().next().filter(|_| !ambiguous), ambiguous)
}

fn find_workspace_root(dirs: &[PathBuf], packages: &[PackageMetadata], fallback: &Path) -> PathBuf {
    dirs.iter()
        .find(|dir| {
            packages
                .iter()
                .find(|package| package.root.as_path() == dir.as_path())
                .is_some_and(|package| package.workspaces)
                || valid_pnpm_workspace_file(&dir.join("pnpm-workspace.yaml"))
        })
        .cloned()
        .or_else(|| packages.first().map(|item| item.root.clone()))
        .unwrap_or_else(|| fallback.to_path_buf())
}

fn detect_manager(dirs: &[PathBuf], packages: &[PackageMetadata]) -> PackageManager {
    for dir in dirs {
        if let Some(manager) = packages
            .iter()
            .find(|package| package.root.as_path() == dir.as_path())
            .and_then(|package| package.package_manager)
        {
            return manager;
        }
        if let Some(manager) = package_manager_hint_in(dir) {
            return manager;
        }
    }
    PackageManager::Npm
}

fn package_manager_hint_in(dir: &Path) -> Option<PackageManager> {
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
}

fn find_framework_config(dirs: &[PathBuf]) -> FrameworkConfigSearch {
    for dir in dirs {
        let configs = framework_configs_in(dir);
        if configs.is_empty() {
            continue;
        }
        if configs.len() == 1 {
            return FrameworkConfigSearch {
                selected: configs.into_iter().next(),
                ambiguous: false,
            };
        }
        return FrameworkConfigSearch {
            selected: None,
            ambiguous: true,
        };
    }
    FrameworkConfigSearch::default()
}

fn framework_configs_in(dir: &Path) -> Vec<FrameworkConfig> {
    [
        ("jest", TestFramework::Jest),
        ("vitest", TestFramework::Vitest),
        ("playwright", TestFramework::Playwright),
    ]
    .into_iter()
    .flat_map(|(prefix, framework)| {
        ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "json"]
            .into_iter()
            .map(move |extension| (prefix, framework, extension))
    })
    .filter_map(|(prefix, framework, extension)| {
        let path = dir.join(format!("{prefix}.config.{extension}"));
        path.is_file().then_some(FrameworkConfig {
            framework,
            root: dir.to_path_buf(),
        })
    })
    .collect()
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

fn framework_without_script(
    package: Option<&PackageMetadata>,
    config: &FrameworkConfigSearch,
) -> Option<TestFramework> {
    if config.ambiguous || package.is_some_and(|item| item.framework_hint_ambiguous) {
        return None;
    }
    match (
        package.and_then(|item| item.framework),
        config.selected.as_ref().map(|item| item.framework),
    ) {
        (Some(package), Some(config)) if package == config => Some(package),
        (Some(_), Some(_)) => None,
        (Some(package), None) => Some(package),
        (None, Some(config)) => Some(config),
        (None, None) => None,
    }
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
