use super::fingerprint::hash_token;
use crate::engines::util::strip_line_comment;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct SymbolId(usize);

#[derive(Debug, Clone, Copy)]
pub(super) struct Token {
    pub(super) symbol: SymbolId,
    pub(super) line: usize,
}

#[derive(Debug)]
struct InternedSymbol {
    text: String,
    hash: u64,
}

#[derive(Debug, Default)]
pub(super) struct TokenInterner {
    symbols: Vec<InternedSymbol>,
    by_hash: HashMap<u64, Vec<SymbolId>>,
}

impl TokenInterner {
    pub(super) fn intern(&mut self, text: String) -> SymbolId {
        let hash = hash_token(&text);
        if let Some(candidates) = self.by_hash.get(&hash)
            && let Some(&symbol) = candidates
                .iter()
                .find(|&&symbol| self.symbol(symbol) == text.as_str())
        {
            return symbol;
        }

        let symbol = SymbolId(self.symbols.len());
        self.symbols.push(InternedSymbol { text, hash });
        self.by_hash.entry(hash).or_default().push(symbol);
        symbol
    }

    pub(super) fn symbol(&self, symbol: SymbolId) -> &str {
        &self.symbols[symbol.0].text
    }

    pub(super) fn hash(&self, symbol: SymbolId) -> u64 {
        self.symbols[symbol.0].hash
    }
}

pub(super) fn tokenize(content: &str, interner: &mut TokenInterner) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut filter = RoutineDeclFilter::default();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment_start(trimmed) {
            continue;
        }
        let code = strip_line_comment(line);
        let trimmed_code = code.trim();
        if trimmed_code.is_empty() {
            continue;
        }
        if filter.is_routine_decl(trimmed_code) {
            continue;
        }
        tokenize_line(&code, index + 1, &mut tokens, interner);
    }
    tokens
}

#[derive(Debug, Default)]
struct RoutineDeclFilter {
    in_decl: bool,
    brace_depth: usize,
    paren_depth: usize,
    bracket_depth: usize,
}

impl RoutineDeclFilter {
    fn is_routine_decl(&mut self, line: &str) -> bool {
        if !self.in_decl {
            if let Some(multiline) = starts_routine_declaration(line) {
                if multiline {
                    self.in_decl = true;
                    self.update_depths(line);
                    if self.is_finished(line) {
                        self.reset();
                    }
                }
                return true;
            }
            false
        } else {
            self.update_depths(line);
            if self.is_finished(line) {
                self.reset();
            }
            true
        }
    }

    fn update_depths(&mut self, line: &str) {
        let mut in_single = false;
        let mut in_double = false;
        let mut in_backtick = false;
        let mut prev_backslash = false;
        for c in line.chars() {
            if prev_backslash {
                prev_backslash = false;
                continue;
            }
            if c == '\\' {
                prev_backslash = true;
                continue;
            }
            if update_quote_state(c, &mut in_single, &mut in_double, &mut in_backtick) {
                continue;
            }
            if in_single || in_double || in_backtick {
                continue;
            }
            adjust_delimiter_depths(self, c);
        }
    }

    fn is_finished(&self, line: &str) -> bool {
        if self.brace_depth > 0 || self.paren_depth > 0 || self.bracket_depth > 0 {
            return false;
        }
        if line.ends_with(';') {
            return true;
        }
        if line.ends_with('\\') {
            return false;
        }
        is_routine_terminal_line(line)
    }

    fn reset(&mut self) {
        self.in_decl = false;
        self.brace_depth = 0;
        self.paren_depth = 0;
        self.bracket_depth = 0;
    }
}

fn update_quote_state(
    c: char,
    in_single: &mut bool,
    in_double: &mut bool,
    in_backtick: &mut bool,
) -> bool {
    match c {
        '\'' if !*in_double && !*in_backtick => {
            *in_single = !*in_single;
            true
        }
        '"' if !*in_single && !*in_backtick => {
            *in_double = !*in_double;
            true
        }
        '`' if !*in_single && !*in_double => {
            *in_backtick = !*in_backtick;
            true
        }
        _ => false,
    }
}

fn adjust_delimiter_depths(tracker: &mut RoutineDeclFilter, c: char) {
    match c {
        '{' => tracker.brace_depth += 1,
        '}' => tracker.brace_depth = tracker.brace_depth.saturating_sub(1),
        '(' => tracker.paren_depth += 1,
        ')' => tracker.paren_depth = tracker.paren_depth.saturating_sub(1),
        '[' => tracker.bracket_depth += 1,
        ']' => tracker.bracket_depth = tracker.bracket_depth.saturating_sub(1),
        _ => {}
    }
}

fn is_routine_terminal_line(line: &str) -> bool {
    // Balanced declarations may end without semicolons. Keep skipping only
    // when an explicit continuation remains; otherwise index subsequent code.
    !line.ends_with(['=', '|', '&', ','])
}

fn starts_routine_declaration(trimmed: &str) -> Option<bool> {
    if is_use_declaration(trimmed)
        || is_import_declaration(trimmed)
        || is_export_reexport_or_type(trimmed)
        || is_type_alias_declaration(trimmed)
    {
        Some(true)
    } else {
        None
    }
}

fn is_use_declaration(trimmed: &str) -> bool {
    if trimmed.starts_with("use ") {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("pub") {
        let rest = rest.trim_start();
        if rest.starts_with("use ") {
            return true;
        }
        if rest.starts_with('(') {
            if let Some(after_paren) = rest.find(')') {
                let after = rest[after_paren + 1..].trim_start();
                if after.starts_with("use ") {
                    return true;
                }
            }
        }
    }
    false
}

fn is_import_declaration(trimmed: &str) -> bool {
    const IMPORT_PREFIXES: &[&str] = &[
        "import ", "import\t", "import{", "import\"", "import'", "import (",
    ];
    if IMPORT_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
        || trimmed == "import ("
        || trimmed == "import"
    {
        return true;
    }
    trimmed.starts_with("from ") && trimmed.contains("import")
}

fn is_export_reexport_or_type(trimmed: &str) -> bool {
    if trimmed.starts_with("export *") {
        return true;
    }
    if trimmed.starts_with("export {") || trimmed.starts_with("export type {") {
        return true;
    }
    if trimmed.starts_with("export ") && trimmed.contains(" from ") {
        return true;
    }
    false
}

fn is_type_alias_declaration(trimmed: &str) -> bool {
    let s = strip_export_and_pub(trimmed);
    let Some(after_type) = s.strip_prefix("type ") else {
        return false;
    };
    let after_type = after_type.trim_start();
    if after_type.starts_with('{') {
        return true;
    }
    if let Some(eq_pos) = after_type.find('=') {
        let before_eq = after_type[..eq_pos].trim();
        return is_valid_type_alias_name(before_eq);
    }
    s.ends_with(';') || s.contains('{')
}

fn strip_export_and_pub(trimmed: &str) -> &str {
    if let Some(rest) = trimmed.strip_prefix("export ") {
        return rest.trim_start();
    }
    let Some(rest) = trimmed.strip_prefix("pub") else {
        return trimmed;
    };
    let rest = rest.trim_start();
    if let Some(stripped) = rest.strip_prefix('(') {
        if let Some(after_paren) = stripped.find(')') {
            return stripped[after_paren + 1..].trim_start();
        }
    }
    rest
}

fn is_valid_type_alias_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '<' | '>' | ',' | ' ' | '[' | ']')
        })
}

fn is_comment_start(trimmed: &str) -> bool {
    trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*")
}

fn tokenize_line(
    line: &str,
    line_num: usize,
    tokens: &mut Vec<Token>,
    interner: &mut TokenInterner,
) {
    let mut chars = line.chars().peekable();
    while let Some(&current) = chars.peek() {
        if current.is_whitespace() {
            chars.next();
        } else {
            let kind = dispatch_char_lex(current, &mut chars);
            tokens.push(Token {
                symbol: interner.intern(kind),
                line: line_num,
            });
        }
    }
}

fn dispatch_char_lex(current: char, chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    if current.is_ascii_alphabetic() || current == '_' {
        lex_word(chars)
    } else if current.is_ascii_digit() {
        lex_number(chars);
        "_LIT_".to_string()
    } else if matches!(current, '"' | '\'' | '`') {
        lex_string(chars, current);
        "_STR_".to_string()
    } else {
        chars.next();
        current.to_string()
    }
}

fn lex_word(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut word = String::new();
    while let Some(&character) = chars.peek() {
        if !character.is_ascii_alphanumeric() && character != '_' {
            break;
        }
        word.push(character);
        chars.next();
    }
    word
}

fn lex_number(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(&character) = chars.peek() {
        if !character.is_ascii_alphanumeric() && character != '.' {
            break;
        }
        chars.next();
    }
}

fn lex_string(chars: &mut std::iter::Peekable<std::str::Chars>, quote: char) {
    chars.next();
    while let Some(&character) = chars.peek() {
        chars.next();
        if character == '\\' {
            chars.next();
        } else if character == quote {
            break;
        }
    }
}
