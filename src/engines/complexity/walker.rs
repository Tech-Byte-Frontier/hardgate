use super::languages::SupportedLanguage;
use std::collections::HashSet;
use tree_sitter::Node;

#[derive(Default)]
pub struct AnalysisState {
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub max_nesting_depth: usize,
    pub statements: usize,
    pub operators: HashSet<String>,
    pub operands: HashSet<String>,
    pub total_operators: usize,
    pub total_operands: usize,
}

impl AnalysisState {
    pub fn new() -> Self {
        Self {
            cyclomatic: 1,
            ..Default::default()
        }
    }
}

pub struct WalkerContext<'a> {
    pub source: &'a [u8],
    pub lang: SupportedLanguage,
}

pub fn walk_node(
    node: Node,
    ctx: &WalkerContext,
    current_nesting: usize,
    state: &mut AnalysisState,
) {
    let kind = node.kind();
    let is_branch = check_branch(kind);

    if is_branch {
        state.cyclomatic += 1;
        state.cognitive += 1 + current_nesting as u32;
    }

    check_boolean_operator(node, ctx.source, kind, state);
    check_statement(kind, state);
    check_halstead(node, ctx.source, kind, state);

    let next_nesting = if is_branch {
        let new_depth = current_nesting + 1;
        if new_depth > state.max_nesting_depth {
            state.max_nesting_depth = new_depth;
        }
        new_depth
    } else {
        current_nesting
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if ctx.lang.is_function_node(child.kind()) {
            continue;
        }
        walk_node(child, ctx, next_nesting, state);
    }
}

fn check_branch(kind: &str) -> bool {
    matches!(
        kind,
        "if_expression"
            | "if_statement"
            | "while_expression"
            | "while_statement"
            | "for_expression"
            | "for_statement"
            | "for_in_statement"
            | "loop_expression"
            | "match_arm"
            | "switch_case"
            | "expression_case"
            | "catch_clause"
            | "ternary_expression"
            | "conditional_expression"
    )
}

fn check_boolean_operator(node: Node, source: &[u8], kind: &str, state: &mut AnalysisState) {
    if kind != "binary_expression" && kind != "boolean_operator" {
        return;
    }
    let Ok(text) = node.utf8_text(source) else { return };
    if text.contains("&&") || text.contains("||") || text.contains(" and ") || text.contains(" or ") {
        state.cyclomatic += 1;
        state.cognitive += 1;
    }
}

fn check_statement(kind: &str, state: &mut AnalysisState) {
    if kind.ends_with("_statement") || kind.ends_with("_expression") {
        state.statements += 1;
    }
}

fn check_halstead(node: Node, source: &[u8], kind: &str, state: &mut AnalysisState) {
    if node.child_count() > 0 {
        return;
    }
    let Ok(text) = node.utf8_text(source) else { return };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    if is_operator(kind, trimmed) {
        state.operators.insert(trimmed.to_string());
        state.total_operators += 1;
    } else if is_operand(kind) {
        state.operands.insert(trimmed.to_string());
        state.total_operands += 1;
    }
}

fn is_operator(kind: &str, text: &str) -> bool {
    matches!(
        text,
        "+" | "-" | "*" | "/" | "%" | "=" | "==" | "!=" | "<" | "<=" | ">" | ">="
            | "&&" | "||" | "!" | "&" | "|" | "^" | "<<" | ">>" | "+=" | "-=" | "*=" | "/="
            | "fn" | "let" | "mut" | "if" | "else" | "match" | "switch" | "case" | "for"
            | "while" | "return" | "break" | "continue" | "try" | "catch" | "def" | "func"
            | "const" | "var" | "function"
    ) || kind.contains("operator")
}

fn is_operand(kind: &str) -> bool {
    kind == "identifier"
        || kind == "property_identifier"
        || kind == "field_identifier"
        || kind.ends_with("_literal")
        || kind == "string"
        || kind == "number"
        || kind == "integer"
        || kind == "float"
}
