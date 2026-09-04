use crate::commands::mutate::MutateOptions;
use crate::config::HardgateConfig;
use crate::discovery::{ClassifiedFile, DiscoverOptions, discover_files};
use crate::engines::mutation::target::is_effective_mutation_target;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Discover, validate, classify, and normalize native mutation targets.
///
/// Mutation always operates on repository-relative paths.  Canonicalizing the
/// root, scopes, and discovered files before classification prevents an
/// absolute path, a parent-directory escape, or a symlink from widening the
/// mutation boundary outside the repository.
pub(super) fn discover_targets(
    opts: &MutateOptions,
    config: &HardgateConfig,
    root: &Path,
) -> Result<Vec<PathBuf>> {
    let canonical_root = canonical_repository_root(root)?;
    let files = match opts.scoped.as_deref() {
        Some(scope) => {
            let canonical_scope = canonical_scope(scope, &canonical_root)?;
            if canonical_scope.is_file() {
                let classified =
                    ClassifiedFile::new_with_config(&canonical_scope, &config.classification)?;
                if !is_effective_mutation_target(&classified, config) {
                    bail!(
                        "refusing to mutate `{}` because it is classified as {:?}, not production source",
                        scope.display(),
                        classified.role
                    );
                }
                if !classified.ast_supported {
                    bail!(
                        "refusing to mutate `{}` because Hardgate has no AST mutator for its file type",
                        scope.display()
                    );
                }
                return Ok(vec![repository_relative(
                    &canonical_scope,
                    &canonical_root,
                )?]);
            }
            if canonical_scope.is_dir() {
                discover_files(DiscoverOptions {
                    root: &canonical_scope,
                    diff_only: false,
                    exclusions: &config.budgets.files.exclusions.paths,
                })?
            } else {
                bail!(
                    "Mutation scope is neither a file nor a directory: `{}`",
                    scope.display()
                )
            }
        }
        None => discover_files(DiscoverOptions {
            root: &canonical_root,
            diff_only: opts.diff,
            exclusions: &config.budgets.files.exclusions.paths,
        })?,
    };
    filter_production_sources(files, config, &canonical_root)
}

pub(super) fn effective_mutation_target(path: &Path, config: &HardgateConfig) -> Result<bool> {
    let classified = ClassifiedFile::new_with_config(path, &config.classification)?;
    Ok(is_effective_mutation_target(&classified, config))
}

fn filter_production_sources(
    files: Vec<PathBuf>,
    config: &HardgateConfig,
    canonical_root: &Path,
) -> Result<Vec<PathBuf>> {
    let mut targets = Vec::new();
    for path in files {
        let canonical = canonical_file(&path, canonical_root)?;
        let classified = ClassifiedFile::new_with_config(&canonical, &config.classification)?;
        if is_effective_mutation_target(&classified, config) && classified.ast_supported {
            targets.push(repository_relative(&canonical, canonical_root)?);
        }
    }
    targets.sort();
    targets.dedup();
    Ok(targets)
}

fn canonical_repository_root(root: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(root).with_context(|| {
        format!(
            "Failed to canonicalize repository root `{}`",
            root.display()
        )
    })?;
    if !canonical.is_dir() {
        bail!(
            "Mutation repository root is not a directory: `{}`",
            root.display()
        );
    }
    Ok(canonical)
}

fn canonical_scope(scope: &Path, canonical_root: &Path) -> Result<PathBuf> {
    let candidate = if scope.is_absolute() {
        scope.to_path_buf()
    } else {
        canonical_root.join(scope)
    };
    ensure_lexically_contained(&candidate, canonical_root, scope)?;
    if !candidate.exists() {
        bail!("Path not found: `{}`", scope.display());
    }
    let canonical = fs::canonicalize(&candidate).with_context(|| {
        format!(
            "Failed to canonicalize mutation scope `{}`",
            scope.display()
        )
    })?;
    ensure_contained(&canonical, canonical_root, scope)?;
    Ok(canonical)
}

fn canonical_file(path: &Path, canonical_root: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path).with_context(|| {
        format!(
            "Failed to canonicalize mutation target `{}`",
            path.display()
        )
    })?;
    ensure_contained(&canonical, canonical_root, path)?;
    if !canonical.is_file() {
        bail!("Mutation target is not a file: `{}`", path.display());
    }
    Ok(canonical)
}

fn repository_relative(path: &Path, canonical_root: &Path) -> Result<PathBuf> {
    let relative = path.strip_prefix(canonical_root).with_context(|| {
        format!(
            "Mutation target `{}` is outside repository root `{}`",
            path.display(),
            canonical_root.display()
        )
    })?;
    if relative.as_os_str().is_empty() {
        bail!("Mutation target resolves to the repository root");
    }
    Ok(normalize_relative(relative))
}

fn ensure_contained(path: &Path, canonical_root: &Path, original: &Path) -> Result<()> {
    if path.starts_with(canonical_root) {
        return Ok(());
    }
    bail!(
        "refusing mutation path `{}` because it resolves outside repository root `{}`",
        original.display(),
        canonical_root.display()
    )
}

fn ensure_lexically_contained(path: &Path, canonical_root: &Path, original: &Path) -> Result<()> {
    let normalized = normalize_absolute(path);
    if normalized.starts_with(canonical_root) {
        return Ok(());
    }
    bail!(
        "refusing mutation path `{}` because it escapes repository root `{}`",
        original.display(),
        canonical_root.display()
    )
}

fn normalize_absolute(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

fn normalize_relative(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        if let Component::Normal(value) = component {
            normalized.push(value);
        }
    }
    normalized
}
