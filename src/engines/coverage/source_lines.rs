use std::collections::BTreeSet;

/// Keep changed lines that contain source code rather than whitespace,
/// comments, or delimiter-only syntax. Lines outside the available source are
/// retained so a stale report cannot silently pass by hiding the line.
pub(crate) fn retain_code_lines(content: &str, candidates: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut retained = BTreeSet::new();
    let mut state = ScannerState::default();
    let line_count = content.split('\n').count();
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        let (code, literal) = strip_comments(line, &mut state);
        if candidates.contains(&line_number) && (literal || is_code_bearing(&code)) {
            retained.insert(line_number);
        }
    }
    retained.extend(
        candidates
            .iter()
            .copied()
            .filter(|line| *line == 0 || *line > line_count),
    );
    retained
}

fn strip_comments(line: &str, state: &mut ScannerState) -> (String, bool) {
    let scanner = CommentScanner::new(line, *state);
    let (result, next, literal) = scanner.run();
    *state = next;
    (result, literal)
}

#[derive(Clone, Copy, Default)]
struct ScannerState {
    block_depth: usize,
    literal: Option<LiteralState>,
}

#[derive(Clone, Copy)]
enum LiteralState {
    Quoted(char),
    Raw(usize),
}

struct CommentScanner {
    chars: Vec<char>,
    index: usize,
    state: ScannerState,
    literal_line: bool,
    output: String,
}

impl CommentScanner {
    fn new(line: &str, state: ScannerState) -> Self {
        Self {
            chars: line.chars().collect(),
            index: 0,
            literal_line: state.literal.is_some(),
            state,
            output: String::new(),
        }
    }

    fn run(mut self) -> (String, ScannerState, bool) {
        while self.index < self.chars.len() {
            self.step();
        }
        (self.output, self.state, self.literal_line)
    }

    fn step(&mut self) {
        if self.state.block_depth > 0 {
            self.consume_block();
        } else if let Some(literal) = self.state.literal {
            self.literal_line = true;
            self.consume_literal(literal);
        } else {
            self.consume_code();
        }
    }

    fn consume_block(&mut self) {
        if self.starts_with('/', '*') {
            self.state.block_depth = self.state.block_depth.saturating_add(1);
            self.index += 2;
        } else if self.starts_with('*', '/') {
            self.state.block_depth -= 1;
            self.index += 2;
        } else {
            self.index += 1;
        }
    }

    fn consume_literal(&mut self, literal: LiteralState) {
        match literal {
            LiteralState::Quoted(delimiter) => self.consume_quoted(delimiter),
            LiteralState::Raw(hashes) => self.consume_raw(hashes),
        }
    }

    fn consume_quoted(&mut self, delimiter: char) {
        let character = self.chars[self.index];
        self.output.push(character);
        match character {
            '\\' => self.consume_escape(),
            value if value == delimiter => {
                self.state.literal = None;
                self.index += 1;
            }
            _ => self.index += 1,
        }
    }

    fn consume_raw(&mut self, hashes: usize) {
        if self.chars[self.index] == '"' && self.raw_close_len(hashes).is_some() {
            let length = hashes + 1;
            self.output
                .extend(self.chars[self.index..=self.index + hashes].iter());
            self.index += length;
            self.state.literal = None;
        } else {
            self.output.push(self.chars[self.index]);
            self.index += 1;
        }
    }

    fn consume_escape(&mut self) {
        self.index += 1;
        if let Some(next) = self.chars.get(self.index) {
            self.output.push(*next);
            self.index += 1;
        }
    }

    fn consume_code(&mut self) {
        let character = self.chars[self.index];
        if let Some(hashes) = self.raw_prefix_hashes() {
            let length = hashes + 2;
            self.output
                .extend(self.chars[self.index..self.index + length].iter());
            self.index += length;
            self.state.literal = Some(LiteralState::Raw(hashes));
            self.literal_line = true;
            return;
        }
        match character {
            '\'' | '"' | '`' => {
                self.state.literal = Some(LiteralState::Quoted(character));
                self.literal_line = true;
                self.output.push(character);
                self.index += 1;
            }
            '/' if self.starts_with('/', '/') => self.index = self.chars.len(),
            '#' if is_hash_comment_start(&self.chars, self.index) => self.index = self.chars.len(),
            '/' if self.starts_with('/', '*') => {
                self.state.block_depth = 1;
                self.index += 2;
            }
            _ => {
                self.output.push(character);
                self.index += 1;
            }
        }
    }

    fn starts_with(&self, first: char, second: char) -> bool {
        self.chars.get(self.index) == Some(&first)
            && self.chars.get(self.index + 1) == Some(&second)
    }

    fn raw_prefix_hashes(&self) -> Option<usize> {
        if self.chars.get(self.index) != Some(&'r') {
            return None;
        }
        let mut cursor = self.index + 1;
        while self.chars.get(cursor) == Some(&'#') {
            cursor += 1;
        }
        (self.chars.get(cursor) == Some(&'"')).then_some(cursor - self.index - 1)
    }

    fn raw_close_len(&self, hashes: usize) -> Option<usize> {
        let end = self.index + hashes + 1;
        if end > self.chars.len() {
            return None;
        }
        (self.chars[self.index + 1..end]
            .iter()
            .all(|character| *character == '#'))
        .then_some(hashes + 1)
    }
}

fn is_hash_comment_start(chars: &[char], index: usize) -> bool {
    if chars.get(index + 1) == Some(&'[') || chars.get(index + 1) == Some(&'!') {
        return false;
    }
    if index == 0 {
        return true;
    }
    chars[index - 1].is_whitespace() && chars.get(index + 1) != Some(&'[')
}

fn is_code_bearing(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with("//")
        || trimmed.starts_with('#') && !trimmed.starts_with("#[") && !trimmed.starts_with("#!")
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("--")
    {
        return false;
    }
    !trimmed.chars().all(|character| {
        matches!(
            character,
            '{' | '}' | '[' | ']' | '(' | ')' | ';' | ',' | ':'
        )
    })
}

#[cfg(test)]
#[path = "source_lines_tests.rs"]
mod tests;
