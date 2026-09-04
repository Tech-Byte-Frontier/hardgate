use super::fingerprint::hash_token;
use super::tokenizer::{Token, tokenize};
use globset::GlobSet;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub(super) const MAX_WINDOWS_PER_HASH: usize = 64;
pub(super) const MAX_RAW_MATCHES: usize = 50_000;

/// Failure raised when a bounded clone index would discard evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneIndexError {
    /// A hash bucket already held its representative locations and another
    /// window candidate would have been discarded.
    HashWindowCapacityExceeded {
        file: PathBuf,
        line: usize,
        limit: usize,
    },
    /// The raw-match store reached its limit and another verified match was
    /// found.
    RawMatchCapacityExceeded { limit: usize },
}

impl fmt::Display for CloneIndexError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HashWindowCapacityExceeded { file, line, limit } => write!(
                out,
                "hash-window candidate capacity exhausted at `{}:{line}` (limit {limit})",
                file.display()
            ),
            Self::RawMatchCapacityExceeded { limit } => {
                write!(out, "raw clone-match capacity exhausted (limit {limit})")
            }
        }
    }
}

impl std::error::Error for CloneIndexError {}

#[derive(Debug, Clone)]
pub(super) struct TokenLocation {
    pub(super) file: PathBuf,
    pub(super) stream_idx: usize,
    pub(super) start_line: usize,
    pub(super) end_line: usize,
    pub(super) start_idx: usize,
    pub(super) end_idx: usize,
}

#[derive(Debug, Clone)]
pub(super) struct RawCloneMatch {
    pub(super) file_a: PathBuf,
    pub(super) stream_idx_a: usize,
    pub(super) start_a: usize,
    pub(super) end_a: usize,
    pub(super) start_idx_a: usize,
    pub(super) end_idx_a: usize,
    pub(super) file_b: PathBuf,
    pub(super) stream_idx_b: usize,
    pub(super) start_b: usize,
    pub(super) end_b: usize,
    pub(super) start_idx_b: usize,
    pub(super) end_idx_b: usize,
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
    min_lines: usize,
    min_tokens: usize,
}

struct WindowCheck<'a> {
    location: &'a TokenLocation,
    hash: u64,
    min_lines: usize,
    token_streams: &'a [(PathBuf, Vec<Token>)],
}

pub(super) struct CloneIndex {
    pub(super) token_streams: Vec<(PathBuf, Vec<Token>)>,
    pub(super) raw_matches: Vec<RawCloneMatch>,
}

pub(super) struct CloneIndexOptions<'a> {
    pub(super) files: &'a [(PathBuf, String)],
    pub(super) root: &'a Path,
    pub(super) exclude_glob: Option<&'a GlobSet>,
    pub(super) min_lines: usize,
    pub(super) min_tokens: usize,
    pub(super) changed_files: &'a [PathBuf],
}

/// Build a deterministic, complete clone index or report the first dropped
/// candidate. Changed paths are prioritized only when supplied by diff mode;
/// ties and full-mode ordering remain lexical by relative path.
pub(super) fn build_index(options: CloneIndexOptions<'_>) -> Result<CloneIndex, CloneIndexError> {
    let CloneIndexOptions {
        files,
        root,
        exclude_glob,
        min_lines,
        min_tokens,
        changed_files,
    } = options;
    let changed = changed_files
        .iter()
        .map(|path| relative_path(path, root))
        .collect::<HashSet<_>>();
    let mut inputs: Vec<(PathBuf, String)> = files
        .iter()
        .filter_map(|(abs_path, content)| {
            let rel_path = relative_path(abs_path, root);
            if exclude_glob.is_some_and(|exclude| exclude.is_match(&rel_path)) {
                return None;
            }
            Some((rel_path, content.clone()))
        })
        .collect();
    inputs.sort_by(|(left, _), (right, _)| {
        changed
            .contains(right)
            .cmp(&changed.contains(left))
            .then(left.cmp(right))
    });
    let token_streams = inputs
        .into_iter()
        .map(|(path, content)| (path, tokenize(&content)))
        .collect::<Vec<_>>();
    let mut window_map = HashMap::new();
    let mut raw_matches = Vec::new();
    for (stream_idx, (rel_path, tokens)) in token_streams.iter().enumerate() {
        let mut state = CloneIndexState {
            window_map: &mut window_map,
            raw_matches: &mut raw_matches,
        };
        index_file_windows(
            FileWindowInput {
                tokens,
                rel_path,
                stream_idx,
                token_streams: &token_streams,
                min_lines,
                min_tokens,
            },
            &mut state,
        )?;
    }
    Ok(CloneIndex {
        token_streams,
        raw_matches,
    })
}

fn relative_path(path: &Path, root: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn index_file_windows(
    input: FileWindowInput<'_>,
    state: &mut CloneIndexState,
) -> Result<(), CloneIndexError> {
    let FileWindowInput {
        tokens,
        rel_path,
        stream_idx,
        token_streams,
        min_lines,
        min_tokens,
    } = input;
    if tokens.len() < min_tokens {
        return Ok(());
    }
    const BASE: u64 = 31337;
    let mut rolling_hash = init_rolling_hash(&tokens[..min_tokens], BASE);
    let pow_base = calc_pow_base(min_tokens - 1, BASE);
    let mut i = 0;
    loop {
        let start_line = tokens[i].line;
        let end_line = tokens[i + min_tokens - 1].line;
        let line_span = end_line.saturating_sub(start_line) + 1;
        if line_span >= min_lines {
            let loc = TokenLocation {
                file: rel_path.to_path_buf(),
                stream_idx,
                start_line,
                end_line,
                start_idx: i,
                end_idx: i + min_tokens - 1,
            };
            state.check_and_record(WindowCheck {
                location: &loc,
                hash: rolling_hash,
                min_lines,
                token_streams,
            })?;
        }
        if i + min_tokens >= tokens.len() {
            break;
        }
        let old_token_hash = hash_token(&tokens[i].kind);
        let new_token_hash = hash_token(&tokens[i + min_tokens].kind);
        rolling_hash = rolling_hash
            .wrapping_sub(old_token_hash.wrapping_mul(pow_base))
            .wrapping_mul(BASE)
            .wrapping_add(new_token_hash);
        i += 1;
    }
    Ok(())
}

impl CloneIndexState<'_> {
    fn check_and_record(&mut self, check: WindowCheck<'_>) -> Result<(), CloneIndexError> {
        if let Some(existing) = self.window_map.get_mut(&check.hash) {
            compare_existing_windows(existing, &check, self.raw_matches)?;
            if existing.len() >= MAX_WINDOWS_PER_HASH {
                return Err(CloneIndexError::HashWindowCapacityExceeded {
                    file: check.location.file.clone(),
                    line: check.location.start_line,
                    limit: MAX_WINDOWS_PER_HASH,
                });
            }
            existing.push(check.location.clone());
        } else {
            self.window_map
                .insert(check.hash, vec![check.location.clone()]);
        }
        Ok(())
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
) -> Result<(), CloneIndexError> {
    for previous in existing {
        let same_file = previous.file == check.location.file;
        let too_close = same_file
            && (check.location.start_line <= previous.end_line.saturating_add(check.min_lines));
        if !too_close && token_sequences_match(previous, check.location, check.token_streams) {
            if raw_matches.len() >= MAX_RAW_MATCHES {
                return Err(CloneIndexError::RawMatchCapacityExceeded {
                    limit: MAX_RAW_MATCHES,
                });
            }
            raw_matches.push(raw_clone_match(previous, check.location));
        }
    }
    Ok(())
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

pub(super) fn token_slice(
    stream_idx: usize,
    start_idx: usize,
    end_idx: usize,
    streams: &[(PathBuf, Vec<Token>)],
) -> Option<&[Token]> {
    streams.get(stream_idx)?.1.get(start_idx..=end_idx)
}

pub(super) fn token_kinds_match(left: &[Token], right: &[Token]) -> bool {
    left.iter()
        .map(|token| token.kind.as_str())
        .eq(right.iter().map(|token| token.kind.as_str()))
}
