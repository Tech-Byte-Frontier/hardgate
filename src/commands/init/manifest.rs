use super::detect::has_any;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub(crate) struct ManifestInventory {
    pub(crate) cargo: Vec<PathBuf>,
    pub(crate) packages: Vec<PathBuf>,
    pub(crate) python: Vec<PathBuf>,
    pub(crate) go: Vec<PathBuf>,
    pub(crate) js_config: bool,
    pub(crate) python_config: bool,
}

pub(crate) fn collect_manifests(root: &Path) -> ManifestInventory {
    let mut inventory = ManifestInventory::default();
    collect_manifests_at(root, 3, &mut inventory);
    inventory.cargo.sort();
    inventory.packages.sort();
    inventory.python.sort();
    inventory.go.sort();
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

pub(crate) fn collect_manifests_at(
    directory: &Path,
    depth: usize,
    inventory: &mut ManifestInventory,
) {
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

pub(crate) fn record_manifest(path: &Path, inventory: &mut ManifestInventory) {
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

pub(crate) fn is_pruned_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | "vendor" | "dist" | "build"
            )
        })
}
