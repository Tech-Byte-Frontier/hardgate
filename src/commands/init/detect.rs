use crate::config::OrchestrationConfig;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub(crate) enum Ecosystem {
    Rust,
    JavaScript,
    Python,
    Go,
    Ambiguous,
    Unknown,
}

impl Ecosystem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::JavaScript => "JavaScript/TypeScript",
            Self::Python => "Python",
            Self::Go => "Go",
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
    fn new(ecosystem: Ecosystem) -> Self {
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

    fn add_note(&mut self, message: impl Into<String>) {
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
        Ecosystem::Python => detect_root_python(root, &inventory, &mut detection),
        Ecosystem::Go => detect_root_go(root, &inventory, &mut detection),
        Ecosystem::Ambiguous => {
            detect_ambiguous_ecosystems(root, &inventory, &mut detection);
        }
        Ecosystem::Unknown => detection.add_missing(
            "no supported manifest or configured formatter was detected; configure [orchestration] commands explicitly",
        ),
    }
    add_unconfigured_commands(&mut detection);
    detection
}

fn detect_ambiguous_ecosystems(
    root: &Path,
    inventory: &ManifestInventory,
    detection: &mut Detection,
) {
    let has_js = !inventory.packages.is_empty() || inventory.js_config;
    let has_py = !inventory.python.is_empty() || inventory.python_config;

    let root_js = root_manifest(root, &inventory.packages).is_some() || inventory.js_config;
    let root_py = root_python_manifest(root, inventory).is_some() || inventory.python_config;
    if cfg!(unix)
        && has_js
        && has_py
        && root_js
        && root_py
        && inventory.cargo.is_empty()
        && inventory.go.is_empty()
    {
        let mut js_detect = Detection::new(Ecosystem::JavaScript);
        detect_javascript(root, inventory, &mut js_detect);

        let mut py_detect = Detection::new(Ecosystem::Python);
        detect_root_python(root, inventory, &mut py_detect);

        detection.orchestration.format_check = combine_commands(
            py_detect.orchestration.format_check,
            js_detect.orchestration.format_check,
        );
        detection.orchestration.format = combine_commands(
            py_detect.orchestration.format,
            js_detect.orchestration.format,
        );
        detection.orchestration.lint =
            combine_commands(py_detect.orchestration.lint, js_detect.orchestration.lint);
        detection.orchestration.test_cmd = combine_commands(
            py_detect.orchestration.test_cmd,
            js_detect.orchestration.test_cmd,
        );
        detection.orchestration.timeout_secs = Some(300);
        for missing in js_detect
            .missing_setup
            .into_iter()
            .chain(py_detect.missing_setup)
        {
            detection.add_missing(missing);
        }
        for note in js_detect.notes.into_iter().chain(py_detect.notes) {
            detection.add_note(note);
        }
        if detection.orchestration.format_check.is_none()
            || detection.orchestration.lint.is_none()
            || detection.orchestration.test_cmd.is_none()
        {
            detection.add_missing("combined orchestration requires a detected command for both ecosystems; configure missing commands explicitly");
        }
        detection.add_note(
            "Multi-ecosystem project detected (Python + JavaScript/TypeScript); paired orchestration commands run through POSIX sh",
        );
        return;
    }

    detection.add_missing(
        "multiple supported ecosystems were detected; configure [orchestration] commands explicitly",
    );
}

fn combine_commands(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(a), Some(b)) => {
            let script = format!("{a} && {b}").replace('\'', "'\\''");
            Some(format!("sh -c '{script}'"))
        }
        _ => None,
    }
}

fn detect_root_rust(root: &Path, inventory: &ManifestInventory, detection: &mut Detection) {
    if root_manifest(root, &inventory.cargo).is_some() {
        detect_rust(detection);
    } else {
        add_nested_manifest_missing(detection, "Cargo.toml");
    }
}

fn detect_root_python(root: &Path, inventory: &ManifestInventory, detection: &mut Detection) {
    if root_python_manifest(root, inventory).is_some() || inventory.python_config {
        detect_python(root, detection);
    } else {
        add_nested_manifest_missing(detection, "Python");
    }
}

fn detect_root_go(root: &Path, inventory: &ManifestInventory, detection: &mut Detection) {
    if root_manifest(root, &inventory.go).is_some() {
        detect_go(detection);
    } else {
        add_nested_manifest_missing(detection, "go.mod");
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
        (
            !inventory.python.is_empty() || inventory.python_config,
            Ecosystem::Python,
        ),
        (!inventory.go.is_empty(), Ecosystem::Go),
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
            "cargo clippy --all-targets --all-features -- -D warnings",
            "cargo test --all-targets",
        ],
    );
    detection.add_note("Cargo.toml detected; using Cargo's read-only check and test commands");
}

fn detect_go(detection: &mut Detection) {
    set_detected_commands(
        detection,
        [
            "sh -c 'files=$(gofmt -l .) || exit $?; test -z \"$files\"'",
            "gofmt -w .",
            "go vet ./...",
            "go test ./...",
        ],
    );
    detection.add_note("go.mod detected; using gofmt, go vet, and go test");
}

fn set_detected_commands(detection: &mut Detection, commands: [&str; 4]) {
    detection.orchestration = OrchestrationConfig {
        format_check: Some(commands[0].to_string()),
        format: Some(commands[1].to_string()),
        lint: Some(commands[2].to_string()),
        test_cmd: Some(commands[3].to_string()),
        timeout_secs: Some(300),
    };
}

fn detect_javascript(root: &Path, inventory: &ManifestInventory, detection: &mut Detection) {
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
    if let Some(manager) = package_manager_for(manifest, root, &package, detection) {
        set_script_commands(&mut detection.orchestration, &package.scripts, manager);
    }
    let missing = super::tooling::set_javascript_config_commands(
        manifest.parent().unwrap_or(root),
        &mut detection.orchestration,
    );
    for message in missing {
        detection.add_missing(message);
    }
    detection.orchestration.timeout_secs = Some(300);
    if !package.scripts.is_empty() {
        detection.add_note(
            "package scripts are referenced by package-manager command, without embedding script bodies",
        );
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

fn detect_python(root: &Path, detection: &mut Detection) {
    let pyproject = root.join("pyproject.toml");
    let content = fs::read_to_string(&pyproject).unwrap_or_default();
    let ruff =
        has_toml_table(&content, &["tool", "ruff"]) || has_any(root, &["ruff.toml", ".ruff.toml"]);
    let black = has_toml_table(&content, &["tool", "black"]);
    if ruff {
        detection.orchestration.format_check = Some("ruff format --check .".to_string());
        detection.orchestration.format = Some("ruff format .".to_string());
        detection.orchestration.lint = Some("ruff check .".to_string());
    } else if black {
        detection.orchestration.format_check = Some("black --check .".to_string());
        detection.orchestration.format = Some("black .".to_string());
    }
    if has_toml_table(&content, &["tool", "pytest"]) || has_any(root, &["pytest.ini", "tox.ini"]) {
        detection.orchestration.test_cmd = Some("pytest".to_string());
    }
    detection.orchestration.timeout_secs = Some(300);
    if ruff || black {
        detection.add_note("Python formatter/linter configuration detected in project files");
    }
}

fn root_manifest<'a>(root: &Path, manifests: &'a [PathBuf]) -> Option<&'a Path> {
    manifests
        .iter()
        .find(|path| path.parent().is_some_and(|parent| parent == root))
        .map(PathBuf::as_path)
}

fn root_python_manifest<'a>(root: &Path, inventory: &'a ManifestInventory) -> Option<&'a Path> {
    root_manifest(root, &inventory.python)
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
    scripts: BTreeSet<String>,
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
                        .map(|_| name.clone())
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
    scripts: &BTreeSet<String>,
    manager: String,
) {
    if let Some(script) = first_script(scripts, &["format:check", "fmt:check", "check:format"]) {
        orchestration.format_check = Some(format!("{manager} run {script}"));
    }
    if let Some(script) = first_script(scripts, &["format", "fmt"]) {
        orchestration.format = Some(format!("{manager} run {script}"));
    }
    if let Some(script) = first_script(scripts, &["lint", "check:lint"]) {
        orchestration.lint = Some(format!("{manager} run {script}"));
    }
    if let Some(script) = first_script(scripts, &["test"]) {
        orchestration.test_cmd = Some(format!("{manager} run {script}"));
    }
}

fn first_script<'a>(scripts: &BTreeSet<String>, names: &[&'a str]) -> Option<&'a str> {
    names.iter().find(|name| scripts.contains(**name)).copied()
}

fn has_toml_table(content: &str, path: &[&str]) -> bool {
    let Ok(value) = toml::from_str::<toml::Value>(content) else {
        return false;
    };
    let mut current = &value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return false;
        };
        current = next;
    }
    current.is_table()
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
