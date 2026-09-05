use crate::config::OrchestrationConfig;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

#[derive(Debug, Default)]
struct ManifestInventory {
    cargo: Vec<PathBuf>,
    packages: Vec<PathBuf>,
    python: Vec<PathBuf>,
    go: Vec<PathBuf>,
    js_config: bool,
    python_config: bool,
}

pub(crate) fn detect_project(root: &Path) -> Detection {
    let inventory = collect_manifests(root);
    let ecosystem = classify(&inventory);
    let mut detection = Detection::new(ecosystem);
    match ecosystem {
        Ecosystem::Rust => detect_rust(&mut detection),
        Ecosystem::JavaScript => detect_javascript(root, &inventory, &mut detection),
        Ecosystem::Python => detect_python(root, &inventory, &mut detection),
        Ecosystem::Go => detect_go(&mut detection),
        Ecosystem::Ambiguous => detection.add_missing(
            "multiple supported ecosystems were detected; configure [orchestration] commands explicitly",
        ),
        Ecosystem::Unknown => detection.add_missing(
            "no supported manifest or configured formatter was detected; configure [orchestration] commands explicitly",
        ),
    }
    add_unconfigured_commands(&mut detection);
    detection
}

pub(crate) fn legacy_reference_status(root: &Path, branch: &str) -> ReferenceStatus {
    let Some(root) = root.to_str() else {
        return ReferenceStatus::Unknown;
    };
    let reference = format!("{branch}^{{commit}}");
    match Command::new("git")
        .args(["-C", root, "rev-parse", "--verify"])
        .arg(&reference)
        .output()
    {
        Ok(output) if output.status.success() => ReferenceStatus::Available,
        Ok(_) => ReferenceStatus::Missing,
        Err(_) => ReferenceStatus::Unknown,
    }
}

fn collect_manifests(root: &Path) -> ManifestInventory {
    let mut inventory = ManifestInventory::default();
    collect_manifests_at(root, 3, &mut inventory);
    inventory.js_config = has_any(
        root,
        &[
            "biome.json",
            "biome.jsonc",
            "prettier.config.js",
            "prettier.config.cjs",
            "prettier.config.mjs",
            ".prettierrc",
            ".prettierrc.json",
            ".eslintrc",
            ".eslintrc.json",
            ".eslintrc.js",
            "eslint.config.js",
            "eslint.config.mjs",
            "oxlint.config.js",
        ],
    );
    inventory.python_config = has_any(
        root,
        &[
            "ruff.toml",
            ".ruff.toml",
            ".flake8",
            "tox.ini",
            "pytest.ini",
        ],
    );
    inventory
}

fn collect_manifests_at(directory: &Path, depth: usize, inventory: &mut ManifestInventory) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_file() {
            record_manifest(&path, inventory);
        } else if file_type.is_dir() && !is_pruned_directory(&path) {
            collect_manifests_at(&path, depth.saturating_sub(1), inventory);
        }
    }
}

fn record_manifest(path: &Path, inventory: &mut ManifestInventory) {
    match path.file_name().and_then(|name| name.to_str()) {
        Some("Cargo.toml") => inventory.cargo.push(path.to_path_buf()),
        Some("package.json") => inventory.packages.push(path.to_path_buf()),
        Some("pyproject.toml") | Some("setup.py") | Some("requirements.txt") => {
            inventory.python.push(path.to_path_buf())
        }
        Some("go.mod") => inventory.go.push(path.to_path_buf()),
        _ => {}
    }
}

fn is_pruned_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | "vendor" | "dist" | "build"
            )
        })
}

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
        ["gofmt -l .", "gofmt -w .", "go vet ./...", "go test ./..."],
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
    let Some(manifest) = choose_manifest(root, &inventory.packages) else {
        detection.add_missing(
            "a JavaScript/TypeScript package manifest is nested or ambiguous; add explicit [orchestration] commands",
        );
        return;
    };
    let Some(package) = read_package(manifest) else {
        detection.add_missing(
            "package.json could not be read as a JSON object; add explicit [orchestration] commands",
        );
        return;
    };
    if package.manager_invalid {
        detection.add_missing(
            "package.json declares an unsupported package manager; add explicit [orchestration] commands",
        );
        return;
    }
    let manager = package
        .manager
        .or_else(|| package_manager(manifest.parent().unwrap_or(root)));
    let Some(manager) = manager else {
        detection.add_missing(
            "package manager could not be identified; add explicit [orchestration] commands",
        );
        return;
    };
    set_script_commands(&mut detection.orchestration, &package.scripts, manager);
    super::tooling::set_javascript_config_commands(
        manifest.parent().unwrap_or(root),
        &mut detection.orchestration,
    );
    detection.orchestration.timeout_secs = Some(300);
    detection.add_note(
        "package scripts are referenced by package-manager command, without embedding script bodies",
    );
}

fn detect_python(root: &Path, inventory: &ManifestInventory, detection: &mut Detection) {
    let project_root = inventory
        .python
        .iter()
        .find_map(|path| path.parent())
        .unwrap_or(root);
    let pyproject = project_root.join("pyproject.toml");
    let content = fs::read_to_string(&pyproject).unwrap_or_default();
    let ruff = has_toml_table(&content, &["tool", "ruff"])
        || has_any(project_root, &["ruff.toml", ".ruff.toml"]);
    let black = has_toml_table(&content, &["tool", "black"]);
    if ruff {
        detection.orchestration.format_check = Some("ruff format --check .".to_string());
        detection.orchestration.format = Some("ruff format .".to_string());
        detection.orchestration.lint = Some("ruff check .".to_string());
    } else if black {
        detection.orchestration.format_check = Some("black --check .".to_string());
        detection.orchestration.format = Some("black .".to_string());
    }
    if has_toml_table(&content, &["tool", "pytest"])
        || has_any(project_root, &["pytest.ini", "tox.ini"])
    {
        detection.orchestration.test_cmd = Some("pytest".to_string());
    }
    detection.orchestration.timeout_secs = Some(300);
    if ruff || black {
        detection.add_note("Python formatter/linter configuration detected in project files");
    }
}

fn choose_manifest<'a>(root: &Path, manifests: &'a [PathBuf]) -> Option<&'a Path> {
    manifests
        .iter()
        .find(|path| path.parent() == Some(root))
        .or_else(|| (manifests.len() == 1).then(|| &manifests[0]))
        .map(PathBuf::as_path)
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
        .map(|scripts| scripts.keys().cloned().collect())
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
    for (name, manager) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("package-lock.json", "npm"),
    ] {
        if root.join(name).is_file() {
            return Some(manager.to_string());
        }
    }
    Some("npm".to_string())
}

fn set_script_commands(
    orchestration: &mut OrchestrationConfig,
    scripts: &BTreeSet<String>,
    manager: String,
) {
    if let Some(script) = first_script(scripts, &["format:check", "fmt:check", "check:format"]) {
        orchestration.format_check = Some(run_script(&manager, script));
    }
    if let Some(script) = first_script(scripts, &["format", "fmt"]) {
        orchestration.format = Some(run_script(&manager, script));
    }
    if let Some(script) = first_script(scripts, &["lint", "check:lint"]) {
        orchestration.lint = Some(run_script(&manager, script));
    }
    if let Some(script) = first_script(scripts, &["test"]) {
        orchestration.test_cmd = Some(run_script(&manager, script));
    }
}

fn first_script<'a>(scripts: &'a BTreeSet<String>, names: &[&str]) -> Option<&'a str> {
    names.iter().find(|name| scripts.contains(*name)).copied()
}

fn run_script(manager: &str, script: &str) -> String {
    format!("{manager} run {script}")
}

fn has_toml_table(content: &str, path: &[&str]) -> bool {
    let Ok(value) = content.parse::<toml::Value>() else {
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
