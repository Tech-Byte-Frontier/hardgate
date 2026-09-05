use crate::config::HardgateConfig;
use crate::discovery::{DiscoverOptions, discover_paths, filter_files_by_paths};
use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct Scope<'a> {
    pub config: &'a HardgateConfig,
    pub diff: bool,
    pub paths: &'a [PathBuf],
    pub root: &'a Path,
    pub full: Option<&'a crate::discovery::DiscoveryResult>,
}

pub(super) fn select_files(
    scope: Scope<'_>,
    discovery: crate::discovery::DiscoveryResult,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let Scope {
        config,
        diff,
        paths,
        root,
        full,
    } = scope;
    let scope_paths = normalize_scope_paths(paths, root)?;
    let crate::discovery::DiscoveryResult {
        files: discovered_files,
        excluded_files: discovered_excluded,
        ..
    } = discovery;

    let (mut files, mut excluded_files) = if diff && !paths.is_empty() {
        let owned;
        let full_discovery = match full {
            Some(full) => full,
            None => {
                owned = discover_paths(DiscoverOptions {
                    root,
                    diff_only: false,
                    exclusions: &config.budgets.files.exclusions.paths,
                })?;
                &owned
            }
        };
        let explicit_files =
            filter_files_by_paths(full_discovery.files.clone(), &scope_paths, root)?;
        let mut files = discovered_files;
        files.extend(explicit_files);
        let mut excluded_files = discovered_excluded;
        excluded_files.extend_from_slice(&full_discovery.excluded_files);
        (files, excluded_files)
    } else {
        (
            filter_files_by_paths(discovered_files, &scope_paths, root)?,
            discovered_excluded,
        )
    };

    files.sort();
    files.dedup();
    excluded_files.sort();
    excluded_files.dedup();

    // Discovery intentionally keeps budget-excluded files in `files`; only
    // report an advisory for excluded files that survived the selected scope.
    // This also removes duplicates when diff and full discoveries overlap.
    let selected: HashSet<String> = files.iter().map(|path| path_key(path)).collect();
    excluded_files.retain(|path| selected.contains(&path_key(path)));

    Ok((files, excluded_files))
}

fn normalize_scope_paths(paths: &[PathBuf], root: &Path) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let absolute_root = fs::canonicalize(root)?;
    paths
        .iter()
        .map(|path| {
            let absolute_path = if path.is_absolute() {
                path.clone()
            } else {
                absolute_root.join(path)
            };
            if !absolute_path.exists() {
                anyhow::bail!("Path not found: {}", path.display());
            }
            let absolute_path = fs::canonicalize(absolute_path)?;
            Ok(absolute_path
                .strip_prefix(&absolute_root)
                .map(PathBuf::from)
                .unwrap_or(absolute_path))
        })
        .collect()
}

fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    value.strip_prefix("./").unwrap_or(&value).to_string()
}
