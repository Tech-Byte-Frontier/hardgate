use super::fingerprint::hash_token;
use crate::engines::util::strip_slash_comment;
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
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment_start(trimmed) {
            continue;
        }
        let code = strip_slash_comment(line);
        tokenize_line(&code, index + 1, &mut tokens, interner);
    }
    tokens
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
