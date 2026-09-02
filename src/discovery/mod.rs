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
