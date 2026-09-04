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
    let files = discover_scope_or_repository(opts, config, &canonical_root)?;
    filter_production_sources(files, config, &canonical_root)
}

pub(super) fn effective_mutation_target(path: &Path, config: &HardgateConfig) -> Result<bool> {
    let classified = ClassifiedFile::new_with_config(path, &config.classification)?;
    Ok(is_effective_mutation_target(&classified, config))
}

fn discover_scope_or_repository(
    opts: &MutateOptions,
    config: &HardgateConfig,
    canonical_root: &Path,
) -> Result<Vec<PathBuf>> {
    match opts.scoped.as_deref() {
        Some(scope) => discover_scoped_files(scope, canonical_root, config),
        None => discover_files(DiscoverOptions {
            root: canonical_root,
            diff_only: opts.diff,
            exclusions: &config.budgets.files.exclusions.paths,
        }),
    }
}

fn discover_scoped_files(
    scope: &Path,
    canonical_root: &Path,
    config: &HardgateConfig,
) -> Result<Vec<PathBuf>> {
    let canonical_scope = canonical_scope(scope, canonical_root)?;
    if canonical_scope.is_file() {
        return scoped_file_target(scope, &canonical_scope, canonical_root, config);
    }
    if !canonical_scope.is_dir() {
        bail!(
            "Mutation scope is neither a file nor a directory: `{}`",
            scope.display()
        );
    }
    discover_files(DiscoverOptions {
        root: &canonical_scope,
        diff_only: false,
        exclusions: &config.budgets.files.exclusions.paths,
    })
}

fn scoped_file_target(
    scope: &Path,
    canonical_scope: &Path,
    canonical_root: &Path,
    config: &HardgateConfig,
) -> Result<Vec<PathBuf>> {
    let relative_scope = repository_relative(canonical_scope, canonical_root)?;
    let classified = ClassifiedFile::new_with_config(&relative_scope, &config.classification)?;
    ensure_scoped_file_target(scope, &relative_scope, &classified, config)?;
    Ok(vec![relative_scope])
}

fn ensure_scoped_file_target(
    scope: &Path,
    relative_scope: &Path,
    classified: &ClassifiedFile,
    config: &HardgateConfig,
) -> Result<()> {
    if !is_effective_mutation_target(classified, config) {
        let builtin_role = ClassifiedFile::new(relative_scope).role;
        bail!(
            "refusing to mutate `{}` because it is classified as {:?}, not production source (built-in role {:?})",
            scope.display(),
            classified.role,
            builtin_role,
        );
    }
    if !classified.ast_supported {
        bail!(
            "refusing to mutate `{}` because Hardgate has no AST mutator for its file type",
            scope.display()
        );
    }
    Ok(())
}

fn filter_production_sources(
    files: Vec<PathBuf>,
    config: &HardgateConfig,
    canonical_root: &Path,
) -> Result<Vec<PathBuf>> {
    let mut targets = Vec::new();
    for path in files {
        let canonical = canonical_file(&path, canonical_root)?;
        let relative = repository_relative(&canonical, canonical_root)?;
        let classified = ClassifiedFile::new_with_config(&relative, &config.classification)?;
        if is_effective_mutation_target(&classified, config) && classified.ast_supported {
            targets.push(relative);
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

fn normalize_relative(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        if let Component::Normal(value) = component {
            normalized.push(value);
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_targets_are_repository_relative_and_normalized() {
        let root = std::env::temp_dir().join(format!(
            "hardgate-mutation-target-unit-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
        fs::write(root.join("src/nested.rs"), "pub fn nested() {}\n").unwrap();

        let options = MutateOptions {
            scoped: Some(PathBuf::from("./src/../src")),
            ..Default::default()
        };
        let targets = discover_targets(&options, &HardgateConfig::default(), &root).unwrap();
        assert_eq!(
            targets,
            vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/nested.rs")]
        );

        fs::remove_dir_all(root).unwrap();
    }
}
