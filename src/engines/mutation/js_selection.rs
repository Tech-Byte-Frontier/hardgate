use super::js_manifest::PackageMetadata;
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
    let package_root = package
        .map(|item| item.root.as_path())
        .unwrap_or(execution_root);
    let bases = direct_test_bases(source, execution_root, package_root);
    for base in deduplicate_paths(bases) {
        if let Some(found) = find_direct_test(&base, &names, package_root) {
            return Some(found);
        }
    }
    for base in deduplicate_paths(test_roots(execution_root, package_root)) {
        if let Some(found) = find_nested_test(&base, &names, 4, package_root) {
            return Some(found);
        }
    }
    None
}
fn direct_test_bases(source: &Path, execution_root: &Path, package_root: &Path) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(parent) = source.parent()
        && parent.starts_with(package_root)
    {
        bases.push(parent.to_path_buf());
        bases.push(parent.join("__tests__"));
    }
    bases.push(package_root.join("tests"));
    bases.push(package_root.join("__tests__"));
    if let Some(parent) = source
        .parent()
        .and_then(|path| path.strip_prefix(package_root).ok())
    {
        bases.push(package_root.join("tests").join(parent));
        bases.push(package_root.join("__tests__").join(parent));
    }
    if execution_root != package_root && execution_root.starts_with(package_root) {
        bases.push(execution_root.join("tests"));
        bases.push(execution_root.join("__tests__"));
    }
    bases
}
fn test_names(stem: &str, source_extension: Option<&str>) -> Vec<String> {
    ["test", "spec"]
        .into_iter()
        .flat_map(|kind| {
            ordered_extensions(source_extension)
                .into_iter()
                .map(move |extension| format!("{stem}.{kind}.{extension}"))
        })
        .collect()
}
fn ordered_extensions(source_extension: Option<&str>) -> Vec<&str> {
    let extensions = ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];
    let source_extension = source_extension.and_then(|source| {
        extensions
            .iter()
            .copied()
            .find(|extension| source.eq_ignore_ascii_case(extension))
    });
    let mut ordered = extensions
        .iter()
        .filter(|extension| Some(**extension) != source_extension)
        .copied()
        .collect::<Vec<_>>();
    if let Some(source) = source_extension {
        ordered.insert(0, source);
    }
    ordered
}
fn test_roots(execution_root: &Path, package_root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![package_root.join("tests"), package_root.join("__tests__")];
    if execution_root != package_root && execution_root.starts_with(package_root) {
        roots.push(execution_root.join("tests"));
        roots.push(execution_root.join("__tests__"));
    }
    roots
}
fn find_direct_test(base: &Path, names: &[String], package_root: &Path) -> Option<PathBuf> {
    if !within_package_root(base, package_root) || is_nested_package(base, package_root) {
        return None;
    }
    names
        .iter()
        .map(|name| base.join(name))
        .find(|path| path.is_file() && within_package_root(path, package_root))
}
fn find_nested_test(
    base: &Path,
    names: &[String],
    depth: usize,
    package_root: &Path,
) -> Option<PathBuf> {
    if !can_search_nested(base, depth, package_root) {
        return None;
    }
    let mut entries = fs::read_dir(base)
        .ok()?
        .filter_map(std::result::Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    entries
        .into_iter()
        .find_map(|entry| find_nested_entry(&entry.path(), names, depth, package_root))
}
fn can_search_nested(base: &Path, depth: usize, package_root: &Path) -> bool {
    depth > 0
        && base.is_dir()
        && within_package_root(base, package_root)
        && !is_nested_package(base, package_root)
}
fn find_nested_entry(
    path: &Path,
    names: &[String],
    depth: usize,
    package_root: &Path,
) -> Option<PathBuf> {
    if is_named_test(path, names, package_root) {
        return Some(path.to_path_buf());
    }
    if !path.is_dir()
        || path == package_root
        || is_nested_package(path, package_root)
        || is_pruned_test_dir(path)
        || !within_package_root(path, package_root)
    {
        return None;
    }
    find_nested_test(path, names, depth.saturating_sub(1), package_root)
}
fn is_named_test(path: &Path, names: &[String], package_root: &Path) -> bool {
    path.is_file()
        && within_package_root(path, package_root)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| names.iter().any(|candidate| candidate == name))
}
fn is_nested_package(path: &Path, package_root: &Path) -> bool {
    path != package_root && fs::symlink_metadata(path.join("package.json")).is_ok()
}
fn within_package_root(path: &Path, package_root: &Path) -> bool {
    let Ok(canonical_root) = fs::canonicalize(package_root) else {
        return false;
    };
    let Ok(canonical_path) = fs::canonicalize(path) else {
        return false;
    };
    canonical_path.starts_with(canonical_root)
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

#[cfg(test)]
#[path = "js_selection_support_tests.rs"]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "js_selection_tests.rs"]
mod tests;
