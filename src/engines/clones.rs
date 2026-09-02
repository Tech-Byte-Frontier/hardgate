use crate::config::CloneConfig;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneViolation {
    pub file_a: PathBuf,
    pub lines_a: (usize, usize),
    pub file_b: PathBuf,
    pub lines_b: (usize, usize),
    pub tokens: usize,
    pub lines: usize,
    pub message: String,
    pub recommendation: String,
}

#[derive(Debug, Clone)]
struct TokenLocation {
    file: PathBuf,
    start_line: usize,
    end_line: usize,
}

#[derive(Debug, Clone)]
struct Token {
    kind: String,
    line: usize,
}

#[derive(Debug, Clone)]
struct RawCloneMatch {
    file_a: PathBuf,
    start_a: usize,
    end_a: usize,
    file_b: PathBuf,
    start_b: usize,
    end_b: usize,
}

struct CloneIndexState<'a> {
    window_map: &'a mut HashMap<u64, Vec<TokenLocation>>,
    raw_matches: &'a mut Vec<RawCloneMatch>,
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

    pub fn detect_clones(&self, files: &[(PathBuf, String)], root: &Path) -> Vec<CloneViolation> {
        let mut window_map: HashMap<u64, Vec<TokenLocation>> = HashMap::new();
        let mut raw_matches: Vec<RawCloneMatch> = Vec::new();

        for (abs_path, content) in files {
            let rel_path = abs_path.strip_prefix(root).unwrap_or(abs_path);
            if let Some(ref exclude) = self.exclude_glob {
                if exclude.is_match(rel_path) {
                    continue;
                }
            }

            let mut state = CloneIndexState {
                window_map: &mut window_map,
                raw_matches: &mut raw_matches,
            };
            self.index_file_windows(content, rel_path, &mut state);
        }

        coalesce_matches(raw_matches, self.min_tokens, self.min_lines)
    }

    fn index_file_windows(&self, content: &str, rel_path: &Path, state: &mut CloneIndexState) {
        let tokens = tokenize(content);
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
                    start_line,
                    end_line,
                };
                check_and_record_window(&loc, rolling_hash, self.min_lines, state);
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

fn check_and_record_window(
    loc: &TokenLocation,
    hash: u64,
    min_lines: usize,
    state: &mut CloneIndexState,
) {
    if let Some(existing) = state.window_map.get_mut(&hash) {
        for prev in existing.iter() {
            let same_file = prev.file == loc.file;
            let too_close = same_file && (loc.start_line <= prev.end_line + min_lines);
            if !too_close {
                state.raw_matches.push(RawCloneMatch {
                    file_a: prev.file.clone(),
                    start_a: prev.start_line,
                    end_a: prev.end_line,
                    file_b: loc.file.clone(),
                    start_b: loc.start_line,
                    end_b: loc.end_line,
                });
            }
        }
        existing.push(loc.clone());
    } else {
        state.window_map.insert(hash, vec![loc.clone()]);
    }
}

fn coalesce_matches(
    mut matches: Vec<RawCloneMatch>,
    min_tokens: usize,
    min_lines: usize,
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
            if last.file_a == m.file_a
                && last.file_b == m.file_b
                && m.start_a <= last.end_a + 2
                && m.start_b <= last.end_b + 2
            {
                last.end_a = last.end_a.max(m.end_a);
                last.end_b = last.end_b.max(m.end_b);
                merged = true;
            }
        }
        if !merged {
            coalesced.push(m);
        }
    }

    coalesced
        .into_iter()
        .filter_map(|c| build_violation(c, min_tokens, min_lines))
        .collect()
}

fn build_violation(
    c: RawCloneMatch,
    min_tokens: usize,
    min_lines: usize,
) -> Option<CloneViolation> {
    let span_a = c.end_a.saturating_sub(c.start_a) + 1;
    let span_b = c.end_b.saturating_sub(c.start_b) + 1;
    let span = span_a.min(span_b);
    if span < min_lines {
        return None;
    }

    Some(CloneViolation {
        file_a: c.file_a.clone(),
        lines_a: (c.start_a, c.end_a),
        file_b: c.file_b.clone(),
        lines_b: (c.start_b, c.end_b),
        tokens: min_tokens,
        lines: span,
        message: format!(
            "Duplicate code clone ({} lines, ~{} tokens) between `{}:{}-{}` and `{}:{}-{}`",
            span,
            min_tokens,
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

fn hash_token(kind: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in kind.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn tokenize(content: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment_start(trimmed) {
            continue;
        }
        tokenize_line(line, line_num, &mut tokens);
    }
    tokens
}

fn is_comment_start(trimmed: &str) -> bool {
    trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*")
}

fn tokenize_line(line: &str, line_num: usize, tokens: &mut Vec<Token>) {
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        dispatch_char_lex(c, &mut chars, line_num, tokens);
    }
}

fn dispatch_char_lex(
    c: char,
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line_num: usize,
    tokens: &mut Vec<Token>,
) {
    if c.is_ascii_alphabetic() || c == '_' {
        let word = lex_word(chars);
        tokens.push(Token {
            kind: word,
            line: line_num,
        });
    } else if c.is_ascii_digit() {
        lex_number(chars);
        tokens.push(Token {
            kind: "_LIT_".to_string(),
            line: line_num,
        });
    } else if c == '"' || c == '\'' || c == '`' {
        lex_string(chars, c);
        tokens.push(Token {
            kind: "_STR_".to_string(),
            line: line_num,
        });
    } else {
        chars.next();
        tokens.push(Token {
            kind: c.to_string(),
            line: line_num,
        });
    }
}

fn lex_word(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut word = String::new();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    word
}

fn lex_number(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_alphanumeric() || ch == '.' {
            chars.next();
        } else {
            break;
        }
    }
}

fn lex_string(chars: &mut std::iter::Peekable<std::str::Chars>, quote: char) {
    chars.next();
    while let Some(&ch) = chars.peek() {
        chars.next();
        if ch == '\\' {
            chars.next();
        } else if ch == quote {
            break;
        }
    }
}
