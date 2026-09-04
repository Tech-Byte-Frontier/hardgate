use crate::config::CloneConfig;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
mod fingerprint;
mod tokenizer;
use fingerprint::{clone_fingerprint, hash_token};
use tokenizer::{Token, tokenize};

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

#[derive(Debug, Clone)]
struct TokenLocation {
    file: PathBuf,
    stream_idx: usize,
    start_line: usize,
    end_line: usize,
    start_idx: usize,
    end_idx: usize,
}

#[derive(Debug, Clone)]
struct RawCloneMatch {
    file_a: PathBuf,
    stream_idx_a: usize,
    start_a: usize,
    end_a: usize,
    start_idx_a: usize,
    end_idx_a: usize,
    file_b: PathBuf,
    stream_idx_b: usize,
    start_b: usize,
    end_b: usize,
    start_idx_b: usize,
    end_idx_b: usize,
}

struct CloneIndexState<'a> {
    window_map: &'a mut HashMap<u64, Vec<TokenLocation>>,
    raw_matches: &'a mut Vec<RawCloneMatch>,
}

struct FileWindowInput<'a> {
    tokens: &'a [Token],
    rel_path: &'a Path,
    stream_idx: usize,
    token_streams: &'a [(PathBuf, Vec<Token>)],
}

struct WindowCheck<'a> {
    location: &'a TokenLocation,
    hash: u64,
    min_lines: usize,
    token_streams: &'a [(PathBuf, Vec<Token>)],
}

// Repeated generated constructs can create O(n^2) comparisons for one hash.
// A bounded bucket still reports representative clones without allowing an
// adversarial repeated token stream to exhaust memory or CPU.
const MAX_WINDOWS_PER_HASH: usize = 64;
const MAX_RAW_MATCHES: usize = 50_000;

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
                let rel_path = abs_path.strip_prefix(root).unwrap_or(abs_path);
                if exclude.is_match(rel_path) {
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
    pub fn detect_clones(&self, files: &[(PathBuf, String)], root: &Path) -> Vec<CloneViolation> {
        let mut window_map: HashMap<u64, Vec<TokenLocation>> = HashMap::new();
        let mut raw_matches: Vec<RawCloneMatch> = Vec::new();
        let token_streams: Vec<(PathBuf, Vec<Token>)> = files
            .iter()
            .filter_map(|(abs_path, content)| {
                let rel_path = abs_path.strip_prefix(root).unwrap_or(abs_path);
                if self
                    .exclude_glob
                    .as_ref()
                    .is_some_and(|exclude| exclude.is_match(rel_path))
                {
                    return None;
                }
                Some((rel_path.to_path_buf(), tokenize(content)))
            })
            .collect();
        for (stream_idx, (rel_path, tokens)) in token_streams.iter().enumerate() {
            let mut state = CloneIndexState {
                window_map: &mut window_map,
                raw_matches: &mut raw_matches,
            };
            self.index_file_windows(
                FileWindowInput {
                    tokens,
                    rel_path,
                    stream_idx,
                    token_streams: &token_streams,
                },
                &mut state,
            );
            if raw_matches.len() >= MAX_RAW_MATCHES {
                break;
            }
        }
        coalesce_matches(raw_matches, self.min_tokens, self.min_lines, &token_streams)
    }

    fn index_file_windows(&self, input: FileWindowInput<'_>, state: &mut CloneIndexState) {
        let FileWindowInput {
            tokens,
            rel_path,
            stream_idx,
            token_streams,
        } = input;
        if tokens.len() < self.min_tokens {
            return;
        }
        const BASE: u64 = 31337;
        let mut rolling_hash = init_rolling_hash(&tokens[..self.min_tokens], BASE);
        let pow_base = calc_pow_base(self.min_tokens - 1, BASE);
        let mut i = 0;
        loop {
            let start_line = tokens[i].line;
            let end_line = tokens[i + self.min_tokens - 1].line;
            let line_span = end_line.saturating_sub(start_line) + 1;
            if line_span >= self.min_lines {
                let loc = TokenLocation {
                    file: rel_path.to_path_buf(),
                    stream_idx,
                    start_line,
                    end_line,
                    start_idx: i,
                    end_idx: i + self.min_tokens - 1,
                };
                state.check_and_record(WindowCheck {
                    location: &loc,
                    hash: rolling_hash,
                    min_lines: self.min_lines,
                    token_streams,
                });
                if state.raw_matches.len() >= MAX_RAW_MATCHES {
                    return;
                }
            }
            if i + self.min_tokens >= tokens.len() {
                break;
            }
            let old_token_hash = hash_token(&tokens[i].kind);
            let new_token_hash = hash_token(&tokens[i + self.min_tokens].kind);
            rolling_hash = rolling_hash
                .wrapping_sub(old_token_hash.wrapping_mul(pow_base))
                .wrapping_mul(BASE)
                .wrapping_add(new_token_hash);
            i += 1;
        }
    }
}

impl CloneIndexState<'_> {
    fn check_and_record(&mut self, check: WindowCheck<'_>) {
        if let Some(existing) = self.window_map.get_mut(&check.hash) {
            compare_existing_windows(existing, &check, self.raw_matches);
            if existing.len() < MAX_WINDOWS_PER_HASH {
                existing.push(check.location.clone());
            }
        } else {
            self.window_map
                .insert(check.hash, vec![check.location.clone()]);
        }
    }
}

fn init_rolling_hash(slice: &[Token], base: u64) -> u64 {
    let mut h = 0u64;
    for t in slice {
        h = h.wrapping_mul(base).wrapping_add(hash_token(&t.kind));
    }
    h
}

fn calc_pow_base(exp: usize, base: u64) -> u64 {
    let mut p = 1u64;
    for _ in 0..exp {
        p = p.wrapping_mul(base);
    }
    p
}

fn compare_existing_windows(
    existing: &[TokenLocation],
    check: &WindowCheck<'_>,
    raw_matches: &mut Vec<RawCloneMatch>,
) {
    for previous in existing {
        let same_file = previous.file == check.location.file;
        let too_close = same_file
            && (check.location.start_line <= previous.end_line.saturating_add(check.min_lines));
        if !too_close && token_sequences_match(previous, check.location, check.token_streams) {
            raw_matches.push(raw_clone_match(previous, check.location));
            if raw_matches.len() >= MAX_RAW_MATCHES {
                return;
            }
        }
    }
}

fn raw_clone_match(previous: &TokenLocation, location: &TokenLocation) -> RawCloneMatch {
    let (a, b) = if location_ordering(previous, location).is_le() {
        (previous, location)
    } else {
        (location, previous)
    };
    RawCloneMatch {
        file_a: a.file.clone(),
        stream_idx_a: a.stream_idx,
        start_a: a.start_line,
        end_a: a.end_line,
        start_idx_a: a.start_idx,
        end_idx_a: a.end_idx,
        file_b: b.file.clone(),
        stream_idx_b: b.stream_idx,
        start_b: b.start_line,
        end_b: b.end_line,
        start_idx_b: b.start_idx,
        end_idx_b: b.end_idx,
    }
}
fn location_ordering(left: &TokenLocation, right: &TokenLocation) -> std::cmp::Ordering {
    left.file
        .cmp(&right.file)
        .then(left.stream_idx.cmp(&right.stream_idx))
        .then(left.start_idx.cmp(&right.start_idx))
}
fn token_sequences_match(
    left: &TokenLocation,
    right: &TokenLocation,
    streams: &[(PathBuf, Vec<Token>)],
) -> bool {
    token_slice(left.stream_idx, left.start_idx, left.end_idx, streams)
        .zip(token_slice(
            right.stream_idx,
            right.start_idx,
            right.end_idx,
            streams,
        ))
        .is_some_and(|(left, right)| token_kinds_match(left, right))
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
fn token_slice(
    stream_idx: usize,
    start_idx: usize,
    end_idx: usize,
    streams: &[(PathBuf, Vec<Token>)],
) -> Option<&[Token]> {
    streams.get(stream_idx)?.1.get(start_idx..=end_idx)
}
fn token_kinds_match(left: &[Token], right: &[Token]) -> bool {
    left.iter()
        .map(|token| token.kind.as_str())
        .eq(right.iter().map(|token| token.kind.as_str()))
}
