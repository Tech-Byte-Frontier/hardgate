use crate::config::CloneConfig;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
mod fingerprint;
mod index;
mod tokenizer;
use fingerprint::clone_fingerprint;
pub use index::CloneIndexError;
use index::{CloneIndexOptions, RawCloneMatch, build_index, token_kinds_match, token_slice};
use tokenizer::Token;

/// Return a stable repository-relative key without touching the filesystem.
/// Lexical normalization keeps deleted or otherwise nonexistent paths safe to
/// compare while treating `./src/a.rs`, `src/a.rs`, and an absolute path under
/// a relative `.` root as the same repository path.
pub(crate) fn repository_relative_path(path: &Path, root: &Path) -> PathBuf {
    let normalized_path = normalize_path(path);
    let normalized_root = normalize_path(root);
    if normalized_path.is_absolute() {
        let absolute_root = if normalized_root.is_absolute() {
            Some(normalized_root.clone())
        } else {
            std::env::current_dir()
                .ok()
                .map(|cwd| normalize_path(&cwd.join(&normalized_root)))
        };
        if let Some(absolute_root) = absolute_root
            && let Ok(relative) = normalized_path.strip_prefix(&absolute_root)
        {
            return normalize_path(relative);
        }
        return normalized_path;
    }
    if normalized_root.as_os_str().is_empty() {
        return normalized_path;
    }
    normalized_path
        .strip_prefix(&normalized_root)
        .map(normalize_path)
        .unwrap_or(normalized_path)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        append_component(&mut normalized, component);
    }
    normalized
}

fn append_component(normalized: &mut PathBuf, component: std::path::Component<'_>) {
    match component {
        std::path::Component::CurDir => {}
        std::path::Component::ParentDir => normalize_parent(normalized),
        _ => normalized.push(component.as_os_str()),
    }
}

fn normalize_parent(path: &mut PathBuf) {
    if parent_can_pop(path) {
        path.pop();
    } else if !path.is_absolute() {
        path.push("..");
    }
}

fn parent_can_pop(path: &Path) -> bool {
    path.components()
        .next_back()
        .is_some_and(|last| matches!(last, std::path::Component::Normal(_)))
}

/// One duplicated block shared between two locations, with span sizes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneViolation {
    pub file_a: PathBuf,
    pub lines_a: (usize, usize),
    pub file_b: PathBuf,
    pub lines_b: (usize, usize),
    pub tokens: usize,
    pub lines: usize,
    /// Stable FNV-1a fingerprint of the normalized clone tokens. It excludes
    /// file paths and physical line locations so rename lineage can be handled
    /// by the adoption key without changing content identity.
    #[serde(default)]
    pub fingerprint: String,
    pub message: String,
    pub recommendation: String,
}

pub struct CloneDetector {
    min_lines: usize,
    min_tokens: usize,
    exclude_glob: Option<GlobSet>,
}

impl CloneDetector {
    pub fn new(config: &CloneConfig) -> Self {
        let exclude_glob = config.excludes.as_ref().map(|excludes| {
            let mut builder = GlobSetBuilder::new();
            for ex in excludes {
                if let Ok(g) = Glob::new(ex) {
                    builder.add(g);
                }
            }
            builder.build().unwrap_or_else(|_| GlobSet::empty())
        });

        Self {
            min_lines: config.min_lines,
            min_tokens: config.min_tokens,
            exclude_glob,
        }
    }

    pub fn excluded_files<'a>(
        &'a self,
        files: &'a [(PathBuf, String)],
        root: &Path,
    ) -> Vec<&'a PathBuf> {
        let Some(ref exclude) = self.exclude_glob else {
            return Vec::new();
        };
        files
            .iter()
            .filter_map(|(abs_path, _)| {
                let rel_path = repository_relative_path(abs_path, root);
                if exclude.is_match(&rel_path) {
                    Some(abs_path)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn count_excluded_files(&self, files: &[(PathBuf, String)], root: &Path) -> usize {
        self.excluded_files(files, root).len()
    }

    /// Rolling-hash clone detection over token streams, honoring excludes.
    /// The result is explicit so index truncation can never become a silent
    /// empty-success response.
    pub fn detect_clones(
        &self,
        files: &[(PathBuf, String)],
        root: &Path,
    ) -> Result<Vec<CloneViolation>, CloneIndexError> {
        self.detect_clones_checked(files, root)
    }

    /// Build a complete clone index and return an explicit error if a bounded
    /// collection would discard any additional evidence.
    pub fn detect_clones_checked(
        &self,
        files: &[(PathBuf, String)],
        root: &Path,
    ) -> Result<Vec<CloneViolation>, CloneIndexError> {
        self.detect_clones_checked_with_changed_files(files, root, &[])
    }

    /// Checked clone detection with changed files prioritized for diff mode.
    /// All files are still indexed and an exhausted cap remains a hard error.
    pub fn detect_clones_checked_with_changed_files(
        &self,
        files: &[(PathBuf, String)],
        root: &Path,
        changed_files: &[PathBuf],
    ) -> Result<Vec<CloneViolation>, CloneIndexError> {
        let index = build_index(CloneIndexOptions {
            files,
            root,
            exclude_glob: self.exclude_glob.as_ref(),
            min_lines: self.min_lines,
            min_tokens: self.min_tokens,
            changed_files,
        })?;
        Ok(coalesce_matches(
            index.raw_matches,
            self.min_tokens,
            self.min_lines,
            &index.token_streams,
        ))
    }
}
fn coalesce_matches(
    mut matches: Vec<RawCloneMatch>,
    min_tokens: usize,
    min_lines: usize,
    token_streams: &[(PathBuf, Vec<Token>)],
) -> Vec<CloneViolation> {
    if matches.is_empty() {
        return Vec::new();
    }
    matches.sort_by(|a, b| {
        a.file_a
            .cmp(&b.file_a)
            .then(a.file_b.cmp(&b.file_b))
            .then(a.start_a.cmp(&b.start_a))
            .then(a.start_b.cmp(&b.start_b))
    });
    let mut coalesced: Vec<RawCloneMatch> = Vec::new();
    for m in matches {
        let mut merged = false;
        if let Some(last) = coalesced.last_mut() {
            let same_pair = last.file_a == m.file_a && last.file_b == m.file_b;
            if matches_can_merge(last, &m, same_pair, token_streams) {
                merge_match(last, &m);
                merged = true;
            } else if same_pair && ranges_overlap(last, &m) {
                // Repeated token streams generate cross-product windows.
                // Drop overlapping alignments rather than extending one side
                // with an unverified sequence.
                continue;
            }
        }
        if !merged {
            coalesced.push(m);
        }
    }
    coalesced
        .into_iter()
        .filter_map(|c| build_violation(c, min_tokens, min_lines, token_streams))
        .collect()
}
fn ranges_overlap(left: &RawCloneMatch, right: &RawCloneMatch) -> bool {
    let overlap_a = right.start_idx_a <= left.end_idx_a && right.end_idx_a >= left.start_idx_a;
    let overlap_b = right.start_idx_b <= left.end_idx_b && right.end_idx_b >= left.start_idx_b;
    overlap_a || overlap_b
}
fn matches_can_merge(
    left: &RawCloneMatch,
    right: &RawCloneMatch,
    same_pair: bool,
    streams: &[(PathBuf, Vec<Token>)],
) -> bool {
    same_pair
        && left.stream_idx_a == right.stream_idx_a
        && left.stream_idx_b == right.stream_idx_b
        && right.start_a <= left.end_a.saturating_add(2)
        && right.start_b <= left.end_b.saturating_add(2)
        && right.start_idx_a <= left.end_idx_a.saturating_add(1)
        && right.start_idx_b <= left.end_idx_b.saturating_add(1)
        && merged_ranges_match(left, right, streams)
}
fn merge_match(left: &mut RawCloneMatch, right: &RawCloneMatch) {
    left.end_a = left.end_a.max(right.end_a);
    left.end_b = left.end_b.max(right.end_b);
    left.end_idx_a = left.end_idx_a.max(right.end_idx_a);
    left.end_idx_b = left.end_idx_b.max(right.end_idx_b);
    left.start_idx_a = left.start_idx_a.min(right.start_idx_a);
    left.start_idx_b = left.start_idx_b.min(right.start_idx_b);
}
fn build_violation(
    c: RawCloneMatch,
    min_tokens: usize,
    min_lines: usize,
    token_streams: &[(PathBuf, Vec<Token>)],
) -> Option<CloneViolation> {
    let span_a = c.end_a.saturating_sub(c.start_a) + 1;
    let span_b = c.end_b.saturating_sub(c.start_b) + 1;
    let span = span_a.min(span_b);
    if span < min_lines {
        return None;
    }
    let tokens_a = token_slice(c.stream_idx_a, c.start_idx_a, c.end_idx_a, token_streams)?;
    let tokens_b = token_slice(c.stream_idx_b, c.start_idx_b, c.end_idx_b, token_streams)?;
    if !token_kinds_match(tokens_a, tokens_b) {
        return None;
    }
    // Actual token span of the merged clone, not the config threshold.
    let actual_tokens = tokens_a.len();
    if actual_tokens < min_tokens {
        return None;
    }
    Some(CloneViolation {
        file_a: c.file_a.clone(),
        lines_a: (c.start_a, c.end_a),
        file_b: c.file_b.clone(),
        lines_b: (c.start_b, c.end_b),
        tokens: actual_tokens,
        lines: span,
        fingerprint: clone_fingerprint(tokens_a),
        message: format!(
            "Duplicate code clone ({} lines, ~{} tokens) between `{}:{}-{}` and `{}:{}-{}`",
            span,
            actual_tokens,
            c.file_a.display(),
            c.start_a,
            c.end_a,
            c.file_b.display(),
            c.start_b,
            c.end_b
        ),
        recommendation: format!(
            "Extract duplicated logic in `{}` and `{}` into a shared helper.",
            c.file_a.display(),
            c.file_b.display()
        ),
    })
}
fn merged_ranges_match(
    left: &RawCloneMatch,
    right: &RawCloneMatch,
    token_streams: &[(PathBuf, Vec<Token>)],
) -> bool {
    token_slice(
        left.stream_idx_a,
        left.start_idx_a.min(right.start_idx_a),
        left.end_idx_a.max(right.end_idx_a),
        token_streams,
    )
    .zip(token_slice(
        left.stream_idx_b,
        left.start_idx_b.min(right.start_idx_b),
        left.end_idx_b.max(right.end_idx_b),
        token_streams,
    ))
    .is_some_and(|(left, right)| token_kinds_match(left, right))
}
