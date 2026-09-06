use serde::{Deserialize, Serialize};
use std::ops::Range;
use tree_sitter::Node;

/// Physical footprint split by syntax, for review rather than a new policy.
/// A line containing both code and a comment appears in both relevant counts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SizeBreakdown {
    pub physical_lines: usize,
    pub code_lines: usize,
    pub documentation_lines: usize,
    pub comment_lines: usize,
    pub blank_lines: usize,
}

impl SizeBreakdown {
    pub fn description(&self) -> String {
        format!(
            "{} code, {} documentation, {} comment, {} blank lines",
            self.code_lines, self.documentation_lines, self.comment_lines, self.blank_lines
        )
    }
}

pub(super) fn measure_projection(node: Node<'_>, source: &[u8], original: &[u8]) -> SizeBreakdown {
    let mut size = measure_range(node, source, node.byte_range());
    let padding = original[node.byte_range()]
        .split(|byte| *byte == b'\n')
        .zip(source[node.byte_range()].split(|byte| *byte == b'\n'))
        .filter(|(original, visible)| {
            original.iter().any(|byte| !byte.is_ascii_whitespace())
                && visible.iter().all(u8::is_ascii_whitespace)
        })
        .count();
    size.physical_lines = size.physical_lines.saturating_sub(padding);
    size.blank_lines = size.blank_lines.saturating_sub(padding);
    size
}

pub(super) fn measure_file(node: Node<'_>, source: &[u8]) -> SizeBreakdown {
    measure_range(node, source, 0..source.len())
}

fn measure_range(node: Node<'_>, source: &[u8], range: Range<usize>) -> SizeBreakdown {
    let mut comments = Vec::new();
    collect_comments(node, source, &mut comments);
    let start = range.start;
    let text = &source[range];
    let mut size = SizeBreakdown::default();
    let mut range_index = 0;
    let mut bits = 0;
    for (index, byte) in text.iter().copied().enumerate() {
        while comments
            .get(range_index)
            .is_some_and(|(range, _)| start + index >= range.end)
        {
            range_index += 1;
        }
        if let Some((_, documentation)) = comments
            .get(range_index)
            .filter(|(range, _)| range.contains(&(start + index)))
        {
            bits |= if *documentation { 2 } else { 4 };
        } else if !byte.is_ascii_whitespace() {
            bits |= 1;
        }
        if byte == b'\n' {
            count_line(&mut size, bits);
            bits = 0;
        }
    }
    if !text.is_empty() && text.last() != Some(&b'\n') {
        count_line(&mut size, bits);
    }
    size
}

fn count_line(size: &mut SizeBreakdown, bits: u8) {
    size.physical_lines += 1;
    size.code_lines += usize::from(bits & 1 != 0);
    size.documentation_lines += usize::from(bits & 2 != 0);
    size.comment_lines += usize::from(bits & 4 != 0);
    size.blank_lines += usize::from(bits == 0);
}

fn collect_comments(node: Node<'_>, source: &[u8], comments: &mut Vec<(Range<usize>, bool)>) {
    if matches!(node.kind(), "comment" | "line_comment" | "block_comment") {
        let text = &source[node.byte_range()];
        if text.iter().all(u8::is_ascii_whitespace) {
            return;
        }
        let documentation = text.starts_with(b"///") && !text.starts_with(b"////")
            || text.starts_with(b"//!")
            || text.starts_with(b"/**")
            || text.starts_with(b"/*!");
        comments.push((node.byte_range(), documentation));
        return;
    }
    for child in node.children(&mut node.walk()) {
        collect_comments(child, source, comments);
    }
}
