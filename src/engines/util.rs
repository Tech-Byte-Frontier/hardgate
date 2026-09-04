//! Shared string/comment helpers for engines.
//!
//! Centralized here so clone detection does not flag the engines themselves
//! for duplicating quote-tracking and `//`-stripping logic.

/// True if `prefix` (text before a match) ends inside an unclosed string
/// literal. Counts unescaped single/double quotes; odd => inside.
pub fn is_inside_string(prefix: &str) -> bool {
    let (single, double) = count_unclosed_quotes(prefix);
    single % 2 == 1 || double % 2 == 1
}

/// True if byte `offset` in `line` falls inside a string literal.
pub fn is_offset_inside_string(line: &str, offset: usize) -> bool {
    let end = offset.min(line.len());
    // Ensure we don't split a multi-byte char: walk char boundaries.
    let mut cut = end;
    while cut > 0 && !line.is_char_boundary(cut) {
        cut -= 1;
    }
    is_inside_string(&line[..cut])
}

fn count_unclosed_quotes(prefix: &str) -> (usize, usize) {
    let mut single = 0;
    let mut double = 0;
    let mut prev_backslash = false;
    for c in prefix.chars() {
        (single, double, prev_backslash) = step_quote_count(c, single, double, prev_backslash);
    }
    (single, double)
}

fn step_quote_count(c: char, single: usize, double: usize, escaped: bool) -> (usize, usize, bool) {
    if escaped {
        return (single, double, false);
    }
    match c {
        '\\' => (single, double, true),
        '\'' => (single + 1, double, false),
        '"' => (single, double + 1, false),
        _ => (single, double, false),
    }
}

struct QuoteState {
    single: bool,
    double: bool,
    backtick: bool,
}

impl QuoteState {
    fn new() -> Self {
        Self {
            single: false,
            double: false,
            backtick: false,
        }
    }

    fn consume_escape(&self) -> bool {
        self.single || self.double || self.backtick
    }

    fn toggle(&mut self, c: char) {
        match c {
            '\'' if !self.double && !self.backtick => self.single = !self.single,
            '"' if !self.single && !self.backtick => self.double = !self.double,
            '`' if !self.single && !self.double => self.backtick = !self.backtick,
            _ => {}
        }
    }

    fn in_code(&self) -> bool {
        !self.single && !self.double && !self.backtick
    }
}

/// Strip trailing `//` comment outside string literals.
/// Returns the code prefix (owned to keep call sites simple).
pub fn strip_slash_comment(line: &str) -> String {
    match find_comment_start(line, false) {
        Some(i) => line[..i].to_string(),
        None => line.to_string(),
    }
}

/// Strip trailing `//` or `#` comments outside strings.
/// Rust `#[attr]` is preserved as code (not treated as `#` comment).
pub fn strip_line_comment(line: &str) -> String {
    match find_comment_start(line, true) {
        Some(i) => line[..i].to_string(),
        None => line.to_string(),
    }
}

fn find_comment_start(line: &str, hash: bool) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut q = QuoteState::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '\\' && q.consume_escape() {
            i += 2;
            continue;
        }
        q.toggle(c);
        if q.in_code() && is_comment_at(bytes, i, hash) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_comment_at(bytes: &[u8], i: usize, hash: bool) -> bool {
    let c = bytes[i] as char;
    if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
        return true;
    }
    hash && c == '#' && is_hash_comment_start(bytes, i)
}

fn is_hash_comment_start(bytes: &[u8], i: usize) -> bool {
    // `#` starts a shell/Python comment only at line start or after whitespace,
    // and not as part of Rust `#[attr]` / `#!`.
    if bytes.get(i + 1) == Some(&b'[')
        || (bytes.get(i + 1) == Some(&b'!') && bytes.get(i + 2) == Some(&b'['))
    {
        return false;
    }
    if i > 0 {
        let prev = bytes[i - 1] as char;
        if prev == '[' || prev == '!' || prev == '#' {
            return false;
        }
        if !bytes[i - 1].is_ascii_whitespace() {
            return false;
        }
    }
    true
}
