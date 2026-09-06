//! Split declarative data/markup from executable clone candidates. Boundaries
//! remain separate streams so removed syntax cannot join unrelated statements.
use super::tokenizer::{Token, TokenInterner, tokenize};
use crate::engines::complexity::SupportedLanguage;
use std::ops::Range;
use std::path::Path;
use tree_sitter::Node;

pub(super) fn streams(path: &Path, source: &str, interner: &mut TokenInterner) -> Vec<Vec<Token>> {
    let Some((_, tree)) = SupportedLanguage::parse_file(path, source) else {
        // Never hide candidates when parsing cannot establish syntax boundaries.
        return vec![tokenize(source, interner)];
    };
    let newlines = source
        .bytes()
        .enumerate()
        .filter_map(|(index, byte)| (byte == b'\n').then_some(index))
        .collect::<Vec<_>>();
    regions(tree.root_node())
        .into_iter()
        .filter_map(|range| {
            let offset = newlines.partition_point(|index| *index < range.start);
            let mut tokens = tokenize(&source[range], interner);
            for token in &mut tokens {
                token.line += offset;
            }
            (!tokens.is_empty()).then_some(tokens)
        })
        .collect()
}

struct Boundary {
    range: Range<usize>,
    expressions: Vec<Range<usize>>,
}

fn regions(node: Node<'_>) -> Vec<Range<usize>> {
    let mut boundaries = Vec::new();
    collect_boundaries(node, &mut boundaries);
    let mut regions = Vec::new();
    let mut cursor = node.start_byte();
    for boundary in boundaries {
        if cursor < boundary.range.start {
            regions.push(cursor..boundary.range.start);
        }
        regions.extend(boundary.expressions);
        cursor = boundary.range.end;
    }
    if cursor < node.end_byte() {
        regions.push(cursor..node.end_byte());
    }
    regions
}

fn collect_boundaries(node: Node<'_>, boundaries: &mut Vec<Boundary>) {
    if is_data_container(node) && literal_data(node) {
        boundaries.push(Boundary {
            range: node.byte_range(),
            expressions: vec![],
        });
    } else if matches!(
        node.kind(),
        "jsx_element" | "jsx_self_closing_element" | "jsx_fragment"
    ) {
        let mut expressions = Vec::new();
        jsx_expressions(node, &mut expressions);
        boundaries.push(Boundary {
            range: node.byte_range(),
            expressions,
        });
    } else {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_boundaries(child, boundaries);
        }
    }
}

fn jsx_expressions(node: Node<'_>, expressions: &mut Vec<Range<usize>>) {
    let mut cursor = node.walk();
    if node.kind() == "jsx_expression" {
        for child in node.named_children(&mut cursor) {
            if !declarative_reference(child) {
                expressions.extend(regions(child));
            }
        }
    } else {
        for child in node.named_children(&mut cursor) {
            jsx_expressions(child, expressions);
        }
    }
}

fn declarative_reference(node: Node<'_>) -> bool {
    if literal_data(node) {
        return true;
    }
    match node.kind() {
        "identifier" | "property_identifier" | "this" | "comment" => true,
        "member_expression" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor).all(declarative_reference)
        }
        _ => false,
    }
}

fn is_data_container(node: Node<'_>) -> bool {
    matches!(node.kind(), "array" | "array_expression")
        || (matches!(
            node.kind(),
            "object" | "tuple_expression" | "struct_expression"
        ) && node
            .parent()
            .is_some_and(|parent| matches!(parent.kind(), "array" | "array_expression")))
}

fn literal_data(node: Node<'_>) -> bool {
    match node.kind() {
        "string" | "number" | "true" | "false" | "null" | "string_literal"
        | "raw_string_literal" | "integer_literal" | "float_literal" | "boolean_literal"
        | "char_literal" | "comment" | "line_comment" | "block_comment" => true,
        "array" | "array_expression" | "object" | "tuple_expression" | "field_initializer_list" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor).all(literal_data)
        }
        "pair" | "field_initializer" => {
            let key = node
                .child_by_field_name("key")
                .or_else(|| node.child_by_field_name("field"));
            key.is_some_and(|key| {
                matches!(
                    key.kind(),
                    "property_identifier" | "field_identifier" | "string" | "number"
                )
            }) && node.child_by_field_name("value").is_some_and(literal_data)
        }
        "struct_expression" => node.child_by_field_name("body").is_some_and(literal_data),
        _ => false,
    }
}
