use std::collections::BTreeSet;

/// Keep changed lines that contain source code rather than whitespace,
/// comments, or delimiter-only syntax. Lines outside the available source are
/// retained so a stale report cannot silently pass by hiding the line.
pub(crate) fn retain_code_lines(content: &str, candidates: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut retained = BTreeSet::new();
    let mut block_comment = false;
    let line_count = content.split('\n').count();
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        if !candidates.contains(&line_number) {
            continue;
        }
        let code = strip_comments(line, &mut block_comment);
        if is_code_bearing(&code) {
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

fn strip_comments(line: &str, in_block: &mut bool) -> String {
    let scanner = CommentScanner::new(line, *in_block);
    let (result, block) = scanner.run();
    *in_block = block;
    result
}

struct CommentScanner {
    chars: Vec<char>,
    index: usize,
    in_block: bool,
    quote: Option<char>,
    output: String,
}

impl CommentScanner {
    fn new(line: &str, in_block: bool) -> Self {
        Self {
            chars: line.chars().collect(),
            index: 0,
            in_block,
            quote: None,
            output: String::new(),
        }
    }

    fn run(mut self) -> (String, bool) {
        while self.index < self.chars.len() {
            self.step();
        }
        (self.output, self.in_block)
    }

    fn step(&mut self) {
        if self.in_block {
            self.consume_block();
        } else if let Some(delimiter) = self.quote {
            self.consume_quote(delimiter);
        } else {
            self.consume_code();
        }
    }

    fn consume_block(&mut self) {
        if self.starts_with('*', '/') {
            self.in_block = false;
            self.index += 2;
        } else {
            self.index += 1;
        }
    }

    fn consume_quote(&mut self, delimiter: char) {
        let character = self.chars[self.index];
        self.output.push(character);
        match character {
            '\\' => self.consume_escape(),
            value if value == delimiter => {
                self.quote = None;
                self.index += 1;
            }
            _ => self.index += 1,
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
        match character {
            '\'' | '"' | '`' => {
                self.quote = Some(character);
                self.output.push(character);
                self.index += 1;
            }
            '/' if self.starts_with('/', '/') => self.index = self.chars.len(),
            '#' if is_hash_comment_start(&self.chars, self.index) => self.index = self.chars.len(),
            '/' if self.starts_with('/', '*') => {
                self.in_block = true;
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
mod tests {
    use super::retain_code_lines;
    use std::collections::BTreeSet;

    #[test]
    fn excludes_comments_and_delimiters_but_keeps_code() {
        let source = "// comment\n}\nlet answer = 42;\n/* block\n * more\n */\nanswer += 1;\n";
        let lines = BTreeSet::from([1, 2, 3, 4, 5, 6]);
        assert_eq!(retain_code_lines(source, &lines), BTreeSet::from([3]));
    }

    #[test]
    fn retains_lines_not_present_in_source_for_missing_report_detection() {
        let lines = BTreeSet::from([3]);
        assert_eq!(retain_code_lines("one\n", &lines), lines);
    }

    #[test]
    fn ignores_trailing_blank_line() {
        let lines = BTreeSet::from([2]);
        assert!(retain_code_lines("one\n", &lines).is_empty());
    }
}
