use super::js::{PackageManager, TestFramework};
use anyhow::{Context, Result, bail};
use globset::GlobBuilder;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub(crate) struct PackageMetadata {
    pub(crate) root: PathBuf,
    pub(crate) scripts: BTreeMap<String, String>,
    pub(crate) package_manager: Option<PackageManager>,
    pub(crate) framework: Option<TestFramework>,
    pub(crate) framework_hint_ambiguous: bool,
    workspace_patterns: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct PnpmWorkspaceDocument {
    packages: Option<Vec<String>>,
}

pub(crate) fn load_packages(dirs: &[PathBuf]) -> Result<Vec<PackageMetadata>> {
    let mut packages = Vec::new();
    for dir in dirs {
        if let Some(package) = load_package(dir)? {
            packages.push(package);
        }
    }
    Ok(packages)
}

fn load_package(dir: &Path) -> Result<Option<PackageMetadata>> {
    let manifest = dir.join("package.json");
    let Some(metadata) = existing_metadata(&manifest, "JavaScript package manifest")? else {
        return Ok(None);
    };
    if metadata.file_type().is_symlink() {
        bail!(
            "JavaScript package manifest `{}` is a symlink",
            manifest.display()
        );
    }
    if !metadata.file_type().is_file() {
        bail!(
            "JavaScript package manifest `{}` is not a regular file",
            manifest.display()
        );
    }
    Ok(Some(read_package(&manifest, dir)?))
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
    let workspace_patterns = parse_workspaces(object.get("workspaces"), manifest)?;
    Ok(PackageMetadata {
        root: root.to_path_buf(),
        scripts,
        package_manager,
        framework,
        framework_hint_ambiguous,
        workspace_patterns,
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

fn parse_workspaces(value: Option<&Value>, manifest: &Path) -> Result<Option<Vec<String>>> {
    let entries = match value {
        Some(Value::Array(entries)) => Some(entries.as_slice()),
        Some(Value::Object(object)) => object
            .get("packages")
            .and_then(Value::as_array)
            .map(|entries| entries.as_slice()),
        _ => None,
    };
    let Some(entries) = entries else {
        return Ok(None);
    };
    let Some(patterns) = entries
        .iter()
        .map(Value::as_str)
        .map(|pattern| pattern.map(str::trim))
        .collect::<Option<Vec<_>>>()
        .filter(|patterns| {
            !patterns.is_empty() && patterns.iter().all(|pattern| !pattern.is_empty())
        })
    else {
        return Ok(None);
    };
    validate_workspace_patterns(&patterns).with_context(|| {
        format!(
            "JavaScript package manifest `{}` has invalid workspace pattern",
            manifest.display()
        )
    })?;
    Ok(Some(patterns.into_iter().map(str::to_string).collect()))
}

fn validate_workspace_patterns(patterns: &[&str]) -> Result<()> {
    if !patterns.iter().any(|pattern| !pattern.starts_with('!')) {
        bail!("workspace patterns require at least one positive pattern");
    }
    for pattern in patterns {
        let pattern = pattern.strip_prefix('!').unwrap_or(pattern);
        if pattern.is_empty()
            || pattern.contains('\\')
            || Path::new(pattern).components().any(|component| {
                matches!(
                    component,
                    Component::Prefix(_) | Component::RootDir | Component::ParentDir
                )
            })
        {
            bail!("workspace pattern `{pattern}` escapes its package root");
        }
        GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .with_context(|| format!("invalid workspace pattern `{pattern}`"))?;
    }
    Ok(())
}

pub(crate) fn valid_pnpm_workspace_file(path: &Path) -> Result<bool> {
    Ok(read_pnpm_workspace_patterns(path)?.is_some())
}

pub(crate) fn valid_pnpm_workspace_content(content: &str) -> bool {
    parse_pnpm_workspace_content(content).is_ok()
}

pub(crate) fn find_workspace_root(
    dirs: &[PathBuf],
    packages: &[PackageMetadata],
    fallback: &Path,
) -> Result<PathBuf> {
    let package = packages.first();
    for dir in dirs {
        let Some(patterns) = workspace_patterns_at(dir, packages)? else {
            continue;
        };
        if let Some(package) = package {
            if workspace_contains_package(dir, package, &patterns)? {
                return Ok(dir.clone());
            }
        }
    }
    Ok(packages
        .first()
        .map(|item| item.root.clone())
        .unwrap_or_else(|| fallback.to_path_buf()))
}

fn workspace_patterns_at(dir: &Path, packages: &[PackageMetadata]) -> Result<Option<Vec<String>>> {
    let pnpm_patterns = read_pnpm_workspace_patterns(&dir.join("pnpm-workspace.yaml"))?;
    Ok(packages
        .iter()
        .find(|package| package.root.as_path() == dir)
        .and_then(|package| package.workspace_patterns.clone())
        .or(pnpm_patterns))
}

fn workspace_contains_package(
    workspace_root: &Path,
    package: &PackageMetadata,
    patterns: &[String],
) -> Result<bool> {
    if package.root == workspace_root {
        return Ok(true);
    }
    let Ok(relative) = package.root.strip_prefix(workspace_root) else {
        return Ok(false);
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    let mut matched = false;
    for pattern in patterns {
        let excluded = pattern.starts_with('!');
        let pattern = pattern.strip_prefix('!').unwrap_or(pattern);
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .with_context(|| format!("invalid workspace pattern `{pattern}`"))?;
        if glob.compile_matcher().is_match(&relative) {
            if excluded {
                return Ok(false);
            }
            matched = true;
        }
    }
    Ok(matched)
}

fn workspace_test_script<'a>(
    package: Option<&PackageMetadata>,
    packages: &'a [PackageMetadata],
    workspace_root: &Path,
) -> Result<Option<(&'a PackageMetadata, (String, String))>> {
    let Some(package) = package else {
        return Ok(None);
    };
    if package.root == workspace_root {
        return Ok(None);
    }
    let Some(workspace) = packages
        .iter()
        .find(|item| item.root.as_path() == workspace_root)
    else {
        return Ok(None);
    };
    Ok(test_script(workspace)?.map(|script| (workspace, script)))
}

pub(crate) fn manager_for_package(p: &PackageMetadata) -> Option<PackageManager> {
    p.package_manager
        .or_else(|| package_manager_hint_in(&p.root))
}

pub(crate) fn detect_manager(dirs: &[PathBuf], packages: &[PackageMetadata]) -> PackageManager {
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

fn test_script(package: &PackageMetadata) -> Result<Option<(String, String)>> {
    if let Some(command) = package.scripts.get("test") {
        return Ok(Some(("test".to_string(), command.clone())));
    }
    let mut scripts = package
        .scripts
        .iter()
        .filter(|(name, _)| name.starts_with("test:"));
    let Some((name, command)) = scripts.next() else {
        return Ok(None);
    };
    if scripts.next().is_some() {
        bail!(
            "package `{}` defines multiple test:* scripts; configure --test-cmd before mutation",
            package.root.display()
        );
    }
    Ok(Some((name.clone(), command.clone())))
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

fn read_pnpm_workspace_patterns(path: &Path) -> Result<Option<Vec<String>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to inspect pnpm workspace file `{}`", path.display())
            });
        }
    };
    if !metadata.file_type().is_file() {
        bail!(
            "pnpm workspace path `{}` is not a regular file",
            path.display()
        );
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read pnpm workspace file `{}`", path.display()))?;
    let patterns = parse_pnpm_workspace_content(&content)
        .with_context(|| format!("malformed pnpm workspace file `{}`", path.display()))?;
    Ok(Some(patterns))
}

fn parse_pnpm_workspace_content(content: &str) -> Result<Vec<String>> {
    let document = serde_yaml_ng::from_str::<PnpmWorkspaceDocument>(content)
        .context("invalid pnpm workspace YAML")?;
    let patterns = document
        .packages
        .ok_or_else(|| anyhow::anyhow!("pnpm workspace YAML requires a packages list"))?;
    if patterns.is_empty() || patterns.iter().any(|pattern| pattern.trim().is_empty()) {
        bail!("pnpm workspace YAML requires non-empty package patterns");
    }
    let patterns = patterns
        .into_iter()
        .map(|pattern| pattern.trim().to_string())
        .collect::<Vec<_>>();
    let borrowed = patterns.iter().map(String::as_str).collect::<Vec<_>>();
    validate_workspace_patterns(&borrowed)?;
    Ok(patterns)
}

pub(crate) fn existing_metadata(path: &Path, description: &str) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect {description} `{}`", path.display())),
    }
}
