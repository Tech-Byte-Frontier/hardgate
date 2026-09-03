//! Dependency-dir skipping: `node_modules` et al are never gated.
//!
//! A fresh `npx hardgate check` in a JS project must not flag vendored code,
//! with or without `hardgate.toml` and regardless of `.gitignore` state.
//! Hermetic temp trees (deliberately not git repos) so the built-in skip
//! itself — not the ignore crate's gitignore handling — is pinned here.

#[path = "support/fs.rs"]
mod fs;
#[path = "support/trees.rs"]
mod trees;

use fs::tempdir;
use hardgate::discovery::{DiscoverOptions, discover_files_with_exclusions, filter_files_by_paths};
use std::path::PathBuf;
use trees::{has_suffix, write_tree};

/// Temp project with a real source file plus vendored trees per language
/// (JS `node_modules`/`dist`/`build`, Rust `target`, Go `vendor`,
/// Python venvs), including a nested skip dir under `src/`.
fn dep_tree() -> PathBuf {
    let tmp = tempdir("depskip");
    write_tree(
        &tmp,
        &[
            "src/index.js",
            "src/node_modules/nested.js",
            "node_modules/bad/index.js",
            "target/debug/app.rs",
            "dist/bundle.js",
            "build/out.js",
            "vendor/lib.go",
            ".venv/lib/pkg.py",
            "venv/lib/pkg.py",
            "__pycache__/cached.py",
        ],
    );
    tmp
}

#[test]
fn test_dependency_dirs_skipped_without_config() {
    let tmp = dep_tree();
    let result = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: false,
        exclusions: &[],
    })
    .expect("discovery should succeed");

    assert!(
        has_suffix(&result.files, "src/index.js"),
        "project file must be found, got {result:?}"
    );
    for vendored in [
        "node_modules/bad/index.js",
        "src/node_modules/nested.js",
        "target/debug/app.rs",
        "dist/bundle.js",
        "build/out.js",
        "vendor/lib.go",
        ".venv/lib/pkg.py",
        "venv/lib/pkg.py",
        "__pycache__/cached.py",
    ] {
        assert!(
            !has_suffix(&result.files, vendored),
            "vendored file must not be gated: {vendored}"
        );
        assert!(
            !has_suffix(&result.excluded_files, vendored),
            "vendored skip is silent (not an advisory exclusion): {vendored}"
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_skip_matches_exact_dir_names_only() {
    // Prefix lookalikes (`build-output/`) and files carrying a skipped stem
    // (`node_modules.js`) are project code and must still be gated: guards
    // against an over-broad prefix/suffix matcher.
    let tmp = tempdir("depskip-exact");
    write_tree(
        &tmp,
        &[
            "build-output/app.js",
            "src/node_modules.js",
            "node_modules/real-dep/index.js",
        ],
    );
    let result = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: false,
        exclusions: &[],
    })
    .expect("discovery should succeed");

    assert!(has_suffix(&result.files, "build-output/app.js"));
    assert!(has_suffix(&result.files, "src/node_modules.js"));
    assert!(!has_suffix(&result.files, "node_modules/real-dep/index.js"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_explicit_scope_still_reaches_vendored_file() {
    // Escape hatch: explicitly scoped files (and `hardgate scan <file>`,
    // which bypasses discovery) still inspect vendored code on purpose.
    let tmp = dep_tree();
    let discovered = discover_files_with_exclusions(DiscoverOptions {
        root: &tmp,
        diff_only: false,
        exclusions: &[],
    })
    .expect("discovery should succeed");
    let explicit = tmp.join("node_modules/bad/index.js");
    let scoped = filter_files_by_paths(discovered.files, std::slice::from_ref(&explicit), &tmp)
        .expect("explicit scope should succeed");
    assert!(
        scoped.contains(&explicit),
        "explicitly scoped vendored file must be checkable, got {scoped:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
