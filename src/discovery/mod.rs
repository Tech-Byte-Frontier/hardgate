pub mod classification;

pub use classification::{
    AST_EXTENSIONS, ClassifiedFile, FileRole, INVENTORY_EXTENSIONS, ast_supported,
    is_inventory_file,
};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{DirEntry, WalkBuilder};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Backwards-compatible name for extensions included in repository inventory.
pub const SUPPORTED_EXTENSIONS: &[&str] = INVENTORY_EXTENSIONS;

/// Dependency and build-output directories that are never project code to
/// gate (eslint/Biome precedent: `node_modules` is ignored out of the box).
/// Applies with or without `hardgate.toml`, so a fresh `npx hardgate check`
/// never flags vendored code even when it is not gitignored. Unlike user
/// `exclusions` (which surface advisories), this skip is silent like hidden
/// files: dependency trees are not technical debt. `hardgate scan <file>`
/// and explicit `--scoped` paths still inspect such files on purpose.
pub const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    ".venv",
    "venv",
    "__pycache__",
];

/// True when `entry` is a skipped dependency dir: prunes the whole subtree
/// so vendored trees are never descended into. The walker root (depth 0) is
/// always kept so running inside a directory named e.g. `build` still works.
fn is_skipped_dir(entry: &DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_some_and(|t| t.is_dir())
        && SKIPPED_DIRS.contains(&entry.file_name().to_str().unwrap_or_default())
}

/// True when a diff-reported `target` path lives under a skipped
/// dependency dir (mirrors [`is_skipped_dir`] for the git-diff path, where
/// committed-but-vendored files could otherwise slip through).
fn in_skipped_dir(target: &str) -> bool {
    Path::new(target)
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|part| SKIPPED_DIRS.contains(&part))
}

/// Inputs for file discovery: walk `root` (or git diffs with `diff_only`),
/// skipping `exclusions` glob patterns.
pub struct DiscoverOptions<'a> {
    pub root: &'a Path,
    pub diff_only: bool,
    pub exclusions: &'a [String],
}

/// Discovered source files plus files skipped via budget exclusions
/// (reported as technical-debt advisories, never silently dropped).
#[derive(Debug, Clone, Default)]
pub struct DiscoveryResult {
    pub files: Vec<PathBuf>,
    pub excluded_files: Vec<PathBuf>,
    pub classified_files: Vec<ClassifiedFile>,
}

/// Discover source files, returning just the included paths.
pub fn discover_files(options: DiscoverOptions) -> Result<Vec<PathBuf>> {
    discover_files_with_exclusions(options).map(|res| res.files)
}

/// Scope a discovered file list down to explicit CLI path filters.
///
/// Each entry in `paths` may be a file or a directory (relative to `root`
/// or absolute). A discovered file is kept when it equals a file filter or
/// lives under a directory filter. Missing paths are an error so typos fail
/// loudly instead of silently passing the gate.
pub fn filter_files_by_paths(
    files: Vec<PathBuf>,
    paths: &[PathBuf],
    root: &Path,
) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Ok(files);
    }
    let filters = resolve_path_filters(paths, root)?;
    let mut out: Vec<PathBuf> = files
        .into_iter()
        .filter(|f| filters.matches(path_key(f)))
        .collect();
    for f in filters.explicit_files() {
        let key = path_key(f);
        let already = out.iter().any(|e| path_key(e) == key);
        if !already && f.is_file() {
            out.push(f.clone());
        }
    }
    out.sort();
    Ok(out)
}

struct PathFilters {
    files: HashSet<String>,
    dirs: Vec<String>,
    explicit: Vec<PathBuf>,
}

impl PathFilters {
    fn matches(&self, key: String) -> bool {
        self.files.contains(&key) || self.dirs.iter().any(|d| is_within_dir(&key, d))
    }

    fn explicit_files(&self) -> &[PathBuf] {
        &self.explicit
    }
}

fn resolve_path_filters(paths: &[PathBuf], root: &Path) -> Result<PathFilters> {
    let mut files = HashSet::new();
    let mut dirs = Vec::new();
    let mut explicit = Vec::new();
    for p in paths {
        let abs = if p.is_absolute() {
            p.clone()
        } else {
            root.join(p)
        };
        if !abs.exists() {
            anyhow::bail!("Path not found: {}", p.display());
        }
        let rel_key = path_key(&abs);
        let root_key = path_key(&root.join(p));
        if abs.is_file() {
            files.insert(rel_key);
            files.insert(root_key);
            files.insert(path_key(p));
            explicit.push(abs);
        } else {
            dirs.push(rel_key);
            dirs.push(root_key);
            dirs.push(path_key(p));
        }
    }
    Ok(PathFilters {
        files,
        dirs,
        explicit,
    })
}

fn is_within_dir(key: &str, dir: &str) -> bool {
    key == dir || key.starts_with(&format!("{dir}/"))
}

/// Lossy lexical key: forward slashes, no leading `./`.
fn path_key(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

/// Discover source files, keeping excluded ones visible for advisories.
pub fn discover_files_with_exclusions(options: DiscoverOptions) -> Result<DiscoveryResult> {
    let exclusion_glob = build_exclusion_globset(options.exclusions)?;
    let has_exclusions = !options.exclusions.is_empty();

    if options.diff_only {
        return discover_git_diff_files(options.root, &exclusion_glob, has_exclusions);
    }

    let mut files = Vec::new();
    let mut excluded_files = Vec::new();
    let walker = WalkBuilder::new(options.root)
        .standard_filters(true)
        .hidden(true)
        .parents(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|e| !is_skipped_dir(e))
        .build();

    for result in walker {
        let entry = result.context("Failed while walking repository files")?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if is_inventory_file(path) {
            let rel = path.strip_prefix(options.root).unwrap_or(path);
            if has_exclusions && exclusion_glob.is_match(rel) {
                excluded_files.push(path.to_path_buf());
            }
            // A file-budget exclusion belongs only to the budget engine. The
            // file remains visible to classification and every other engine.
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    excluded_files.sort();
    let classified_files = files.iter().map(|path| ClassifiedFile::new(path)).collect();
    Ok(DiscoveryResult {
        files,
        excluded_files,
        classified_files,
    })
}

fn build_exclusion_globset(exclusions: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for ex in exclusions {
        let glob = Glob::new(ex).with_context(|| format!("Invalid exclusion glob `{ex}`"))?;
        builder.add(glob);
    }
    builder.build().context("Failed to compile exclusion globs")
}

fn discover_git_diff_files(
    root: &Path,
    exclusions: &GlobSet,
    has_exclusions: bool,
) -> Result<DiscoveryResult> {
    let mut collector = GitDiffCollector::new(root, exclusions, has_exclusions);
    collector.collect_status()?;
    collector.collect_diff()?;

    let mut files: Vec<PathBuf> = collector.files.into_iter().collect();
    files.sort();
    let mut excluded_files: Vec<PathBuf> = collector.excluded.into_iter().collect();
    excluded_files.sort();
    let classified_files = files.iter().map(|path| ClassifiedFile::new(path)).collect();
    Ok(DiscoveryResult {
        files,
        excluded_files,
        classified_files,
    })
}

struct GitDiffCollector<'a> {
    root: &'a Path,
    exclusions: &'a GlobSet,
    has_exclusions: bool,
    files: HashSet<PathBuf>,
    excluded: HashSet<PathBuf>,
}

impl<'a> GitDiffCollector<'a> {
    fn new(root: &'a Path, exclusions: &'a GlobSet, has_exclusions: bool) -> Self {
        Self {
            root,
            exclusions,
            has_exclusions,
            files: HashSet::new(),
            excluded: HashSet::new(),
        }
    }

    fn run_git_records(&self, args: &[&str]) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.root)
            .output()
            .with_context(|| format!("Failed to execute `git {}`", args.join(" ")))?;
        if !output.status.success() {
            anyhow::bail!(
                "`git {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let stdout = String::from_utf8(output.stdout)
            .context("Git returned a path that was not valid UTF-8")?;
        Ok(stdout
            .split('\0')
            .filter(|record| !record.is_empty())
            .map(str::to_string)
            .collect())
    }

    fn collect_status(&mut self) -> Result<()> {
        let records =
            self.run_git_records(&["status", "--porcelain=v1", "--untracked-files=all", "-z"])?;
        let mut records = records.into_iter();
        while let Some(line) = records.next() {
            let marker = line
                .get(..2)
                .context("Git returned a malformed porcelain status record")?;
            if marker.contains('D') {
                continue;
            }
            let target = line
                .get(3..)
                .context("Git returned a malformed porcelain path record")?;
            self.add_target(target);
            if marker.contains('R') || marker.contains('C') {
                // Porcelain -z emits the old path as a second record. The new
                // path above is the one that exists in the working tree.
                let _ = records.next();
            }
        }
        Ok(())
    }

    fn collect_diff(&mut self) -> Result<()> {
        for target in self.run_git_records(&["diff", "--name-only", "-z", "HEAD"])? {
            self.add_target(&target);
        }
        Ok(())
    }

    fn add_target(&mut self, target: &str) {
        if in_skipped_dir(target) {
            return;
        }
        let path = self.root.join(target);
        if path.is_file() && is_inventory_file(&path) {
            let rel = path.strip_prefix(self.root).unwrap_or(&path);
            if self.has_exclusions && self.exclusions.is_match(rel) {
                self.excluded.insert(path.clone());
            }
            self.files.insert(path);
        }
    }
}
