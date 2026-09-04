use crate::engines::util::strip_slash_comment;

#[derive(Debug, Clone)]
pub(super) struct Token {
    pub(super) kind: String,
    pub(super) line: usize,
}

pub(super) fn tokenize(content: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment_start(trimmed) {
            continue;
        }
        let code = strip_slash_comment(line);
        tokenize_line(&code, index + 1, &mut tokens);
    }
    tokens
}

fn is_comment_start(trimmed: &str) -> bool {
    trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*")
}

fn tokenize_line(line: &str, line_num: usize, tokens: &mut Vec<Token>) {
    let mut chars = line.chars().peekable();
    while let Some(&current) = chars.peek() {
        if current.is_whitespace() {
            chars.next();
        } else {
            dispatch_char_lex(current, &mut chars, line_num, tokens);
        }
    }
}

fn dispatch_char_lex(
    current: char,
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line_num: usize,
    tokens: &mut Vec<Token>,
) {
    let kind = if current.is_ascii_alphabetic() || current == '_' {
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
    };
    tokens.push(Token {
        kind,
        line: line_num,
    });
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
