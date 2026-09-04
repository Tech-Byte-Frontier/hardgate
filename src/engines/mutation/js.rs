use super::js_command::{
    JsCommandInput, build_js_command, framework_from_command, is_exact_bun_test_command,
};
use super::js_manifest::{
    PackageMetadata, detect_manager, existing_metadata, find_workspace_root, load_packages,
    manager_for_package, test_script, workspace_test_script,
};
use super::js_tests::find_relevant_test;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
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
struct ScriptPlan<'a> {
    package: Option<&'a PackageMetadata>,
    script: Option<(String, String)>,
    workspace_fallback: bool,
}
pub(crate) fn resolve_js_test_plan(file: &Path, root: &Path) -> Result<ResolvedTestPlan> {
    let canonical_root = fs::canonicalize(root).with_context(|| {
        format!(
            "failed to canonicalize JavaScript repository root `{}`",
            root.display()
        )
    })?;
    if !canonical_root.is_dir() {
        bail!(
            "JavaScript repository root `{}` is not a directory",
            root.display()
        );
    }
    let source = absolute_source_path(file, &canonical_root)?;
    if !source.starts_with(&canonical_root) {
        bail!(
            "JavaScript source `{}` resolves outside repository root `{}`",
            file.display(),
            canonical_root.display()
        );
    }
    if !source.is_file() {
        bail!("JavaScript source `{}` is not a file", file.display());
    }
    let dirs = ancestor_dirs(source.parent().unwrap_or(&source), &canonical_root);
    let packages = load_packages(&dirs)?;
    let package = packages.first();
    let workspace_root = find_workspace_root(&dirs, &packages, &canonical_root)?;
    let config = find_framework_config(&dirs)?;
    let script_plan = resolve_script_plan(package, &packages, &workspace_root, &config)?;
    let manager = script_plan
        .workspace_fallback
        .then(|| script_plan.package.and_then(manager_for_package))
        .flatten()
        .unwrap_or_else(|| detect_manager(&dirs, &packages));
    let script_package = script_plan.package;
    let script = script_plan.script;
    let framework = script_framework(script.as_ref(), package, &config);
    let execution_root = select_execution_root(
        script_package,
        config.selected.as_ref(),
        script.is_some(),
        &canonical_root,
    );
    let bun_test_script = manager == PackageManager::Bun
        && script
            .as_ref()
            .is_some_and(|(name, command)| name == "test" && is_exact_bun_test_command(command));
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
fn resolve_script_plan<'a>(
    package: Option<&'a PackageMetadata>,
    packages: &'a [PackageMetadata],
    workspace_root: &Path,
    config: &FrameworkConfigSearch,
) -> Result<ScriptPlan<'a>> {
    let local_script = package.map(test_script).transpose()?.flatten();
    if local_script.is_some() {
        return Ok(ScriptPlan {
            package,
            script: local_script,
            workspace_fallback: false,
        });
    }
    if local_framework_signal(package, config) || config.ambiguous {
        return Ok(ScriptPlan {
            package,
            script: None,
            workspace_fallback: false,
        });
    }
    let Some((workspace, script)) = workspace_test_script(package, packages, workspace_root)?
    else {
        return Ok(ScriptPlan {
            package,
            script: None,
            workspace_fallback: false,
        });
    };
    Ok(ScriptPlan {
        package: Some(workspace),
        script: Some(script),
        workspace_fallback: true,
    })
}
fn local_framework_signal(
    package: Option<&PackageMetadata>,
    config: &FrameworkConfigSearch,
) -> bool {
    let package_signal =
        package.is_some_and(|item| item.framework.is_some() || item.framework_hint_ambiguous);
    let config_signal = config
        .selected
        .as_ref()
        .is_some_and(|item| package.is_some_and(|package| item.root.starts_with(&package.root)));
    package_signal || config_signal
}
fn script_framework(
    script: Option<&(String, String)>,
    package: Option<&PackageMetadata>,
    config: &FrameworkConfigSearch,
) -> Option<TestFramework> {
    script
        .map(|(_, command)| framework_from_command(command))
        .unwrap_or_else(|| framework_without_script(package, config))
}
fn absolute_source_path(file: &Path, root: &Path) -> Result<PathBuf> {
    let path = if file.is_absolute() {
        file.to_path_buf()
    } else {
        root.join(file)
    };
    fs::canonicalize(&path).with_context(|| {
        format!(
            "failed to canonicalize JavaScript source `{}`",
            path.display()
        )
    })
}
fn ancestor_dirs(start: &Path, root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut current = start.to_path_buf();
    loop {
        dirs.push(current.clone());
        if current == root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current || !parent.starts_with(root) {
            break;
        }
        current = parent.to_path_buf();
    }
    dirs
}
fn find_framework_config(dirs: &[PathBuf]) -> Result<FrameworkConfigSearch> {
    for dir in dirs {
        let configs = framework_configs_in(dir)?;
        if configs.is_empty() {
            continue;
        }
        if configs.len() == 1 {
            return Ok(FrameworkConfigSearch {
                selected: configs.into_iter().next(),
                ambiguous: false,
            });
        }
        return Ok(FrameworkConfigSearch {
            selected: None,
            ambiguous: true,
        });
    }
    Ok(FrameworkConfigSearch::default())
}
fn framework_configs_in(dir: &Path) -> Result<Vec<FrameworkConfig>> {
    let mut configs = Vec::new();
    for (prefix, framework) in [
        ("jest", TestFramework::Jest),
        ("vitest", TestFramework::Vitest),
        ("playwright", TestFramework::Playwright),
    ] {
        for extension in ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "json"] {
            let path = dir.join(format!("{prefix}.config.{extension}"));
            if let Some(config) = framework_config_at(&path, framework, dir)? {
                configs.push(config);
            }
        }
    }
    Ok(configs)
}
fn framework_config_at(
    path: &Path,
    framework: TestFramework,
    root: &Path,
) -> Result<Option<FrameworkConfig>> {
    let Some(metadata) = existing_metadata(path, "framework config")? else {
        return Ok(None);
    };
    if metadata.file_type().is_symlink() {
        bail!("framework config `{}` is a symlink", path.display());
    }
    Ok(metadata.file_type().is_file().then(|| FrameworkConfig {
        framework,
        root: root.to_path_buf(),
    }))
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

#[cfg(test)]
#[path = "js_resolver_tests.rs"]
mod tests;
