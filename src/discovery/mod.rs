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

pub fn discover_files(options: DiscoverOptions) -> Result<Vec<PathBuf>> {
    let exclusion_glob = build_exclusion_globset(options.exclusions);

    if options.diff_only {
        return discover_git_diff_files(options.root, &exclusion_glob);
    }

    let mut files = Vec::new();
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

        if is_supported_source_file(path, options.root, &exclusion_glob) {
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    Ok(files)
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

fn is_supported_source_file(path: &Path, root: &Path, exclusions: &GlobSet) -> bool {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    if !SUPPORTED_EXTENSIONS.contains(&ext) {
        return false;
    }
    let rel = path.strip_prefix(root).unwrap_or(path);
    !exclusions.is_match(rel)
}

fn discover_git_diff_files(root: &Path, exclusions: &GlobSet) -> Result<Vec<PathBuf>> {
    let mut files = HashSet::new();

    collect_status_files(root, exclusions, &mut files);
    collect_diff_files(root, exclusions, &mut files);

    let mut result: Vec<PathBuf> = files.into_iter().collect();
    result.sort();
    Ok(result)
}

fn collect_status_files(root: &Path, exclusions: &GlobSet, files: &mut HashSet<PathBuf>) {
    let Ok(output) = Command::new("git").args(["status", "--porcelain"]).current_dir(root).output() else {
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if line.len() < 3 || line[0..2].contains('D') {
            continue;
        }
        let raw = line[3..].trim();
        let target = raw.split_once(" -> ").map(|(_, new_p)| new_p.trim()).unwrap_or(raw);
        check_and_add_target(root, target, exclusions, files);
    }
}

fn collect_diff_files(root: &Path, exclusions: &GlobSet, files: &mut HashSet<PathBuf>) {
    let Ok(output) = Command::new("git").args(["diff", "--name-only", "HEAD"]).current_dir(root).output() else {
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        check_and_add_target(root, line.trim(), exclusions, files);
    }
}

fn check_and_add_target(root: &Path, target: &str, exclusions: &GlobSet, files: &mut HashSet<PathBuf>) {
    let path = root.join(target);
    if path.is_file() && is_supported_source_file(&path, root, exclusions) {
        files.insert(path);
    }
}
