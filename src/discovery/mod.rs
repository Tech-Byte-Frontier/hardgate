use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "c", "cpp", "cc", "h", "hpp", "css",
];

pub struct DiscoverOptions<'a> {
    pub root: &'a Path,
    pub diff_only: bool,
    pub exclusions: &'a [String],
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveryResult {
    pub files: Vec<PathBuf>,
    pub excluded_files: Vec<PathBuf>,
}

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

pub fn discover_files_with_exclusions(options: DiscoverOptions) -> Result<DiscoveryResult> {
    let exclusion_glob = build_exclusion_globset(options.exclusions);
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
        .build();

    for result in walker {
        let Ok(entry) = result else { continue };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if is_supported_source_extension(path) {
            let rel = path.strip_prefix(options.root).unwrap_or(path);
            if has_exclusions && exclusion_glob.is_match(rel) {
                excluded_files.push(path.to_path_buf());
            } else {
                files.push(path.to_path_buf());
            }
        }
    }

    files.sort();
    excluded_files.sort();
    Ok(DiscoveryResult {
        files,
        excluded_files,
    })
}

fn build_exclusion_globset(exclusions: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for ex in exclusions {
        if let Ok(g) = Glob::new(ex) {
            builder.add(g);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

fn is_supported_source_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    SUPPORTED_EXTENSIONS.contains(&ext)
}

fn discover_git_diff_files(
    root: &Path,
    exclusions: &GlobSet,
    has_exclusions: bool,
) -> Result<DiscoveryResult> {
    let mut collector = GitDiffCollector::new(root, exclusions, has_exclusions);
    collector.collect_status();
    collector.collect_diff();

    let mut files: Vec<PathBuf> = collector.files.into_iter().collect();
    files.sort();
    let mut excluded_files: Vec<PathBuf> = collector.excluded.into_iter().collect();
    excluded_files.sort();
    Ok(DiscoveryResult {
        files,
        excluded_files,
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

    fn run_git_lines(&self, args: &[&str]) -> Vec<String> {
        let Ok(output) = Command::new("git")
            .args(args)
            .current_dir(self.root)
            .output()
        else {
            return Vec::new();
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.lines().map(|s| s.to_string()).collect()
    }

    fn collect_status(&mut self) {
        for line in self.run_git_lines(&["status", "--porcelain"]) {
            if line.len() < 3 || line[0..2].contains('D') {
                continue;
            }
            let raw = line[3..].trim();
            let target = raw
                .split_once(" -> ")
                .map(|(_, new_p)| new_p.trim())
                .unwrap_or(raw);
            self.add_target(target);
        }
    }

    fn collect_diff(&mut self) {
        for line in self.run_git_lines(&["diff", "--name-only", "HEAD"]) {
            self.add_target(line.trim());
        }
    }

    fn add_target(&mut self, target: &str) {
        let path = self.root.join(target);
        if path.is_file() && is_supported_source_extension(&path) {
            let rel = path.strip_prefix(self.root).unwrap_or(&path);
            if self.has_exclusions && self.exclusions.is_match(rel) {
                self.excluded.insert(path);
            } else {
                self.files.insert(path);
            }
        }
    }
}
