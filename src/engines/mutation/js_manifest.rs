use super::js::{PackageManager, TestFramework};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub(crate) struct PackageMetadata {
    pub(crate) root: PathBuf,
    pub(crate) scripts: BTreeMap<String, String>,
    pub(crate) package_manager: Option<PackageManager>,
    pub(crate) framework: Option<TestFramework>,
    pub(crate) framework_hint_ambiguous: bool,
    pub(crate) workspaces: bool,
}

#[derive(Deserialize)]
struct PnpmWorkspaceDocument {
    packages: Option<Vec<String>>,
}

pub(crate) fn load_packages(dirs: &[PathBuf]) -> Result<Vec<PackageMetadata>> {
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

fn parse_workspaces(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Array(entries)) => valid_workspace_patterns(entries),
        Some(Value::Object(object)) => object
            .get("packages")
            .and_then(Value::as_array)
            .is_some_and(valid_workspace_patterns),
        _ => false,
    }
}

fn valid_workspace_patterns(entries: &[Value]) -> bool {
    !entries.is_empty()
        && entries.iter().all(|entry| {
            entry
                .as_str()
                .is_some_and(|pattern| !pattern.trim().is_empty())
        })
}

pub(crate) fn valid_pnpm_workspace_file(path: &Path) -> bool {
    path.is_file()
        && fs::read_to_string(path)
            .ok()
            .is_some_and(|content| valid_pnpm_workspace_content(&content))
}

pub(crate) fn valid_pnpm_workspace_content(content: &str) -> bool {
    serde_yaml_ng::from_str::<PnpmWorkspaceDocument>(content)
        .ok()
        .and_then(|document| document.packages)
        .is_some_and(|patterns| {
            !patterns.is_empty() && patterns.iter().all(|pattern| !pattern.trim().is_empty())
        })
}

pub(crate) fn find_workspace_root(
    dirs: &[PathBuf],
    packages: &[PackageMetadata],
    fallback: &Path,
) -> PathBuf {
    dirs.iter()
        .find(|dir| is_workspace_boundary(dir, packages))
        .cloned()
        .or_else(|| packages.first().map(|item| item.root.clone()))
        .unwrap_or_else(|| fallback.to_path_buf())
}

fn is_workspace_boundary(dir: &Path, packages: &[PackageMetadata]) -> bool {
    let package = packages
        .iter()
        .find(|package| package.root.as_path() == dir);
    package.is_some_and(|package| package.workspaces)
        || valid_pnpm_workspace_file(&dir.join("pnpm-workspace.yaml"))
}

fn workspace_test_script<'a>(
    package: Option<&PackageMetadata>,
    packages: &'a [PackageMetadata],
    workspace_root: &Path,
) -> Result<Option<(&'a PackageMetadata, (String, String))>> {
    let Some(package) = package else {
        return Ok(None);
    };
    if package.root == workspace_root || !is_workspace_boundary(workspace_root, packages) {
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
