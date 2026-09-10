use crate::config::OrchestrationConfig;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub(crate) enum Ecosystem {
    Rust,
    JavaScript,
    Ambiguous,
    Unknown,
}

impl Ecosystem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::JavaScript => "JavaScript/TypeScript",
            Self::Ambiguous => "multiple ecosystems",
            Self::Unknown => "unknown ecosystem",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReferenceStatus {
    NotApplicable,
    Available,
    Missing,
    Unknown,
}

#[derive(Debug, Clone)]
pub(crate) struct Detection {
    pub(crate) ecosystem: Ecosystem,
    pub(crate) orchestration: OrchestrationConfig,
    pub(crate) missing_setup: Vec<String>,
    pub(crate) notes: Vec<String>,
}

impl Detection {
    pub(super) fn new(ecosystem: Ecosystem) -> Self {
        Self {
            ecosystem,
            orchestration: OrchestrationConfig::default(),
            missing_setup: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub(crate) fn add_missing(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.missing_setup.contains(&message) {
            self.missing_setup.push(message);
        }
    }

    pub(super) fn add_note(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.notes.contains(&message) {
            self.notes.push(message);
        }
    }
}

pub(crate) use super::manifest::{ManifestInventory, collect_manifests};
#[cfg(test)]
pub(crate) use super::manifest::{collect_manifests_at, is_pruned_directory, record_manifest};

pub(crate) fn detect_project(root: &Path) -> Detection {
    let inventory = collect_manifests(root);
    let ecosystem = classify(&inventory);
    let mut detection = Detection::new(ecosystem);
    match ecosystem {
        Ecosystem::Rust => detect_root_rust(root, &inventory, &mut detection),
        Ecosystem::JavaScript => detect_javascript(root, &inventory, &mut detection),
        Ecosystem::Ambiguous => {
            detection.add_missing("multiple supported ecosystems were detected; configure [orchestration] commands explicitly");
        }
        Ecosystem::Unknown => detection.add_missing(
            "no supported manifest or configured formatter was detected; automatic setup recognizes Rust and JavaScript/TypeScript; configure Python tools explicitly",
        ),
    }
    add_unconfigured_commands(&mut detection);
    detection
}

fn detect_root_rust(root: &Path, inventory: &ManifestInventory, detection: &mut Detection) {
    if root_manifest(root, &inventory.cargo).is_some() {
        detect_rust(detection);
    } else {
        add_nested_manifest_missing(detection, "Cargo.toml");
    }
}

pub(crate) use super::reference::legacy_reference_status;

pub(crate) fn has_any(root: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| root.join(name).is_file())
}

fn classify(inventory: &ManifestInventory) -> Ecosystem {
    let kinds = [
        (!inventory.cargo.is_empty(), Ecosystem::Rust),
        (
            !inventory.packages.is_empty() || inventory.js_config,
            Ecosystem::JavaScript,
        ),
    ]
    .into_iter()
    .filter_map(|(present, kind)| present.then_some(kind))
    .collect::<BTreeSet<_>>();
    match kinds.len() {
        0 => Ecosystem::Unknown,
        1 => kinds.into_iter().next().unwrap_or(Ecosystem::Unknown),
        _ => Ecosystem::Ambiguous,
    }
}

fn detect_rust(detection: &mut Detection) {
    set_detected_commands(
        detection,
        [
            "cargo fmt --all -- --check",
            "cargo fmt --all",
            "cargo clippy --workspace --all-targets --all-features --message-format=json -- -D warnings",
            "cargo test --workspace --all-targets --locked",
        ],
    );
    detection.orchestration.additional_tests = vec!["cargo test --workspace --doc --locked".into()];
    detection.add_note("Cargo: all workspace members/targets and doctests; Clippy enables all features. Declare additional feature checks in orchestration.feature_checks or additional_tests.");
}

fn set_detected_commands(detection: &mut Detection, commands: [&str; 4]) {
    detection.orchestration = OrchestrationConfig {
        format_check: Some(commands[0].to_string()),
        format: Some(commands[1].to_string()),
        lint: Some(commands[2].to_string()),
        test_cmd: Some(commands[3].to_string()),
        timeout_secs: Some(300),
        ..OrchestrationConfig::default()
    };
}

pub(super) fn detect_javascript(
    root: &Path,
    inventory: &ManifestInventory,
    detection: &mut Detection,
) {
    let Some(manifest) = root_manifest(root, &inventory.packages) else {
        detect_javascript_without_manifest(root, inventory, detection);
        return;
    };
    let Some(package) = read_package(manifest) else {
        detection.add_missing(
            "package.json could not be read as a JSON object; add explicit [orchestration] commands",
        );
        return;
    };
    let manager = package_manager_for(manifest, root, &package, detection);
    let missing = super::tooling::set_javascript_config_commands(
        manifest.parent().unwrap_or(root),
        &mut detection.orchestration,
    );
    if let Some(manager) = manager.clone() {
        for message in set_script_commands(&mut detection.orchestration, &package.scripts, manager)
        {
            detection.add_missing(message);
        }
    }
    record_tool_setup(manifest.parent().unwrap_or(root), missing, detection);
    if let Some(script) = first_script(
        &package.scripts,
        &["typecheck", "type-check", "check:types"],
    ) && let Some(manager) = manager
    {
        detection.orchestration.typecheck = Some(format!("{manager} run {script}"));
    } else if root.join("tsconfig.json").is_file() {
        detection.orchestration.typecheck = Some("./node_modules/.bin/tsc --noEmit".into());
    }
    detection.orchestration.timeout_secs = Some(300);
    if !package.scripts.is_empty() {
        detection.add_note("recognized formatter/linter scripts use direct verification commands; custom script semantics require an explicit override");
    }
}

fn tool_resolved(message: &str, orchestration: &crate::config::OrchestrationConfig) -> bool {
    (message.starts_with("formatter:") && orchestration.format_check.is_some())
        || (message.starts_with("linter:") && orchestration.lint.is_some())
}

fn record_tool_setup(root: &Path, missing: Vec<String>, detection: &mut Detection) {
    for message in missing {
        if !tool_resolved(&message, &detection.orchestration) {
            detection.add_missing(message);
        }
    }
    for (role, command) in [
        ("formatter", detection.orchestration.format_check.clone()),
        ("linter", detection.orchestration.lint.clone()),
    ] {
        if let Some(command) = command
            && let Some(tool) = command.split_whitespace().next()
            && super::tooling::local_executable(root, tool).is_none()
        {
            detection.add_missing(format!("{role}: repository-local `{tool}` is unavailable; install declared dependencies before running the generated command"));
        }
    }
}

fn detect_javascript_without_manifest(
    root: &Path,
    inventory: &ManifestInventory,
    detection: &mut Detection,
) {
    if !inventory.js_config {
        add_nested_manifest_missing(detection, "package.json");
        return;
    }
    let missing =
        super::tooling::set_javascript_config_commands(root, &mut detection.orchestration);
    for message in missing {
        detection.add_missing(message);
    }
    detection.orchestration.timeout_secs = Some(300);
}

fn package_manager_for(
    manifest: &Path,
    root: &Path,
    package: &PackageInfo,
    detection: &mut Detection,
) -> Option<String> {
    if package.manager_invalid {
        detection.add_missing(
            "package.json declares an unsupported package manager; add explicit [orchestration] commands",
        );
        return None;
    }
    let manager = package
        .manager
        .clone()
        .or_else(|| package_manager(manifest.parent().unwrap_or(root)));
    if manager.is_none() {
        detection.add_missing(
            "multiple package manager lockfiles were found without packageManager; declare one explicitly",
        );
    }
    manager
}

pub(super) fn root_manifest<'a>(root: &Path, manifests: &'a [PathBuf]) -> Option<&'a Path> {
    manifests
        .iter()
        .find(|path| path.parent().is_some_and(|parent| parent == root))
        .map(PathBuf::as_path)
}

fn add_nested_manifest_missing(detection: &mut Detection, manifest: &str) {
    detection.add_missing(format!(
        "{manifest} was found below the policy root; initialize inside that package or provide explicit [orchestration] overrides"
    ));
}

#[derive(Debug)]
struct PackageInfo {
    manager: Option<String>,
    manager_invalid: bool,
    scripts: BTreeMap<String, String>,
}

fn read_package(manifest: &Path) -> Option<PackageInfo> {
    let content = fs::read_to_string(manifest).ok()?;
    let value = serde_json::from_str::<Value>(&content).ok()?;
    let object = value.as_object()?;
    let manager_value = object.get("packageManager");
    let manager = object
        .get("packageManager")
        .and_then(Value::as_str)
        .and_then(parse_manager);
    let manager_invalid = manager_value.is_some() && manager.is_none();
    let scripts = object
        .get("scripts")
        .and_then(Value::as_object)
        .map(|scripts| {
            scripts
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .as_str()
                        .filter(|command| !command.trim().is_empty())
                        .map(|command| (name.clone(), command.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(PackageInfo {
        manager,
        manager_invalid,
        scripts,
    })
}

fn parse_manager(value: &str) -> Option<String> {
    let manager = value.split('@').next()?.trim();
    matches!(manager, "npm" | "pnpm" | "yarn" | "bun").then(|| manager.to_string())
}

fn package_manager(root: &Path) -> Option<String> {
    let managers = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("package-lock.json", "npm"),
    ]
    .into_iter()
    .filter(|(name, _)| root.join(name).is_file())
    .map(|(_, manager)| manager.to_string())
    .collect::<BTreeSet<_>>();
    match managers.len() {
        0 => Some("npm".to_string()),
        1 => managers.into_iter().next(),
        _ => None,
    }
}

fn set_script_commands(
    orchestration: &mut OrchestrationConfig,
    scripts: &BTreeMap<String, String>,
    manager: String,
) -> Vec<String> {
    let mut missing = Vec::new();
    if let Some(script) = first_script(
        scripts,
        &["format:check", "fmt:check", "check:format", "format", "fmt"],
    ) {
        match super::tooling::script_commands(&scripts[script], true) {
            Some((check, fix)) => {
                orchestration.format_check = Some(check.into());
                orchestration.format = fix.map(str::to_owned);
            }
            None => {
                orchestration.format_check = None;
                orchestration.format = None;
                missing.push(format!("formatter: script `{script}` has custom or ambiguous semantics; set orchestration.format_check explicitly to a read-only command"));
            }
        }
    }
    if let Some(script) = first_script(scripts, &["lint", "check:lint"]) {
        orchestration.lint = super::tooling::script_commands(&scripts[script], false)
            .map(|(check, _)| check.to_string());
        if orchestration.lint.is_none() {
            missing.push(format!("linter: script `{script}` has custom or ambiguous semantics; set orchestration.lint explicitly to a read-only command"));
        }
    }
    if let Some(script) = first_script(scripts, &["test"]) {
        orchestration.test_cmd = Some(format!("{manager} run {script}"));
    }
    missing
}

fn first_script<'a>(scripts: &BTreeMap<String, String>, names: &[&'a str]) -> Option<&'a str> {
    names
        .iter()
        .find(|name| scripts.contains_key(**name))
        .copied()
}

fn add_unconfigured_commands(detection: &mut Detection) {
    if detection.ecosystem == Ecosystem::Ambiguous {
        return;
    }
    if detection.orchestration.format_check.is_none() && detection.orchestration.format.is_none() {
        detection.add_missing(
            "formatter command is not configured; add orchestration.format_check and orchestration.format",
        );
    }
    if detection.orchestration.lint.is_none() {
        detection.add_missing("linter command is not configured; add orchestration.lint");
    }
}
#[cfg(test)]
#[path = "detect_tests.rs"]
mod tests;
