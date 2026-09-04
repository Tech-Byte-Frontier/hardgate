use super::js::PackageMetadata;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn find_relevant_test(
    source: &Path,
    execution_root: &Path,
    package: Option<&PackageMetadata>,
) -> Option<PathBuf> {
    let stem = source.file_stem()?.to_str()?;
    let extension = source.extension().and_then(|value| value.to_str());
    let names = test_names(stem, extension);
    let bases = direct_test_bases(source, execution_root, package);
    for base in deduplicate_paths(bases) {
        if let Some(found) = find_direct_test(&base, &names) {
            return Some(found);
        }
    }
    for base in deduplicate_paths(test_roots(execution_root, package)) {
        if let Some(found) = find_nested_test(&base, &names, 4) {
            return Some(found);
        }
    }
    None
}

fn direct_test_bases(
    source: &Path,
    execution_root: &Path,
    package: Option<&PackageMetadata>,
) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(parent) = source.parent() {
        bases.push(parent.to_path_buf());
        bases.push(parent.join("__tests__"));
    }
    for root in [
        Some(execution_root),
        package.map(|item| item.root.as_path()),
    ]
    .into_iter()
    .flatten()
    {
        bases.push(root.join("tests"));
        bases.push(root.join("__tests__"));
        if let Some(parent) = source
            .parent()
            .and_then(|path| path.strip_prefix(root).ok())
        {
            bases.push(root.join("tests").join(parent));
            bases.push(root.join("__tests__").join(parent));
        }
    }
    bases
}

fn test_names(stem: &str, source_extension: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    for kind in ["test", "spec"] {
        for extension in ordered_extensions(source_extension) {
            names.push(format!("{stem}.{kind}.{extension}"));
        }
    }
    names
}

fn ordered_extensions(source_extension: Option<&str>) -> Vec<&str> {
    let extensions = ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];
    let source_extension = source_extension.map(str::to_ascii_lowercase);
    let mut ordered = Vec::with_capacity(extensions.len());
    if let Some(source) = source_extension.as_deref() {
        if let Some(found) = extensions.iter().find(|extension| **extension == source) {
            ordered.push(*found);
        }
    }
    ordered.extend(
        extensions
            .iter()
            .filter(|extension| Some(**extension) != source_extension.as_deref())
            .copied(),
    );
    ordered
}

fn test_roots(execution_root: &Path, package: Option<&PackageMetadata>) -> Vec<PathBuf> {
    let mut roots = vec![
        execution_root.join("tests"),
        execution_root.join("__tests__"),
    ];
    if let Some(package) = package {
        roots.push(package.root.join("tests"));
        roots.push(package.root.join("__tests__"));
    }
    roots
}

fn find_direct_test(base: &Path, names: &[String]) -> Option<PathBuf> {
    names
        .iter()
        .map(|name| base.join(name))
        .find(|path| path.is_file())
}

fn find_nested_test(base: &Path, names: &[String], depth: usize) -> Option<PathBuf> {
    if depth == 0 || !base.is_dir() {
        return None;
    }
    let mut entries = fs::read_dir(base)
        .ok()?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if is_named_test(&path, names) {
            return Some(path);
        }
        if path.is_dir()
            && !is_pruned_test_dir(&path)
            && let Some(found) = find_nested_test(&path, names, depth - 1)
        {
            return Some(found);
        }
    }
    None
}

fn is_named_test(path: &Path, names: &[String]) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| names.iter().any(|candidate| candidate == name))
}

fn is_pruned_test_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "node_modules" | "dist" | "build" | "vendor"))
}

fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut output = Vec::new();
    for path in paths {
        if !output.contains(&path) {
            output.push(path);
        }
    }
    output
}
