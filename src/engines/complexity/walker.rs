use super::languages::SupportedLanguage;
use serde::{Deserialize, Serialize};
use tree_sitter::Node;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityContribution {
    pub line: usize,
    pub column: usize,
    pub kind: String,
    pub description: String,
    pub score: u32,
}

#[derive(Default)]
pub struct AnalysisState {
    pub cyclomatic: u32,
    pub max_nesting_depth: usize,
    pub statements: usize,
    pub cyclomatic_breakdown: Vec<ComplexityContribution>,
}

impl AnalysisState {
    pub fn new() -> Self {
        Self {
            cyclomatic: 1,
            ..Default::default()
        }
    }
}

pub struct WalkerContext {
    pub lang: SupportedLanguage,
}

pub fn walk_node(
    node: Node,
    ctx: &WalkerContext,
    current_nesting: usize,
    state: &mut AnalysisState,
) {
    walk_projected_node(node, (ctx, None), current_nesting, state);
}

pub(super) fn walk_visible_node(
    node: Node,
    context: (&WalkerContext, &[u8]),
    current_nesting: usize,
    state: &mut AnalysisState,
) {
    walk_projected_node(node, (context.0, Some(context.1)), current_nesting, state);
}

fn walk_projected_node(
    node: Node,
    context: (&WalkerContext, Option<&[u8]>),
    current_nesting: usize,
    state: &mut AnalysisState,
) {
    if context.1.is_some_and(|source| {
        source[node.byte_range()]
            .iter()
            .all(u8::is_ascii_whitespace)
    }) {
        return;
    }
    let ctx = context.0;
    let kind = node.kind();
    let is_branch = check_branch(kind);

    if is_branch {
        let line = node.start_position().row + 1;
        let column = node.start_position().column + 1;
        let branch_desc = human_readable_branch(kind);

        state.cyclomatic += 1;
        state.cyclomatic_breakdown.push(ComplexityContribution {
            line,
            column,
            kind: kind.to_string(),
            description: branch_desc.to_string(),
            score: 1,
        });
    }

    check_boolean_operator(node, kind, state);
    check_statement(kind, state);

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
        // `else if` is another arm at the same nesting level. An `if`
        // inside an explicit else block remains a genuinely nested branch.
        let depth = child_nesting(ctx.lang, kind, child.kind(), next_nesting);
        walk_projected_node(child, context, depth, state);
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
            | "match_expression"
            | "match_arm"
            | "switch_statement"
            | "switch_case"
            | "catch_clause"
            | "ternary_expression"
            | "elif_clause"
            | "except_clause"
            | "case_clause"
            | "conditional_expression"
            | "for_in_clause"
            | "if_clause"
    )
}

fn human_readable_branch(kind: &str) -> &'static str {
    // Table lookup keeps cyclomatic low despite many node kinds.
    const TABLE: &[(&[&str], &str)] = &[
        (
            &["if_expression", "if_statement"],
            "conditional branch (`if`)",
        ),
        (&["while_expression", "while_statement"], "loop (`while`)"),
        (
            &["for_expression", "for_statement", "for_in_statement"],
            "loop (`for`)",
        ),
        (&["loop_expression"], "infinite loop (`loop`)"),
        (&["match_expression"], "pattern match (`match`)"),
        (&["match_arm"], "pattern match arm (`match`)"),
        (&["switch_statement"], "switch (`switch`)"),
        (&["switch_case"], "switch case"),
        (&["catch_clause"], "exception handler (`catch`)"),
        (&["ternary_expression"], "ternary operator (`? :`)"),
    ];
    for (kinds, desc) in TABLE {
        if kinds.contains(&kind) {
            return desc;
        }
    }
    "branching construct"
}

fn check_boolean_operator(node: Node, kind: &str, state: &mut AnalysisState) {
    if !matches!(kind, "binary_expression" | "boolean_operator") {
        return;
    }
    // Only the grammar's direct operator token counts. Nested operands,
    // comments, string literals and identifiers cannot add a second branch.
    let op_label = direct_boolean_operator(node);

    if let Some(op) = op_label {
        let line = node.start_position().row + 1;
        let column = node.start_position().column + 1;
        let desc = format!("boolean operator `{}`", op);

        state.cyclomatic += 1;
        state.cyclomatic_breakdown.push(ComplexityContribution {
            line,
            column,
            kind: "boolean_operator".to_string(),
            description: desc,
            score: 1,
        });
    }
}

fn direct_boolean_operator(node: Node) -> Option<&'static str> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| !child.is_named())
        .find_map(|child| classify_operator_token(child.kind()))
}

fn classify_operator_token(token: &str) -> Option<&'static str> {
    // Single table keeps branch count low.
    const OPS: &[(&str, &str)] = &[("&&", "&&"), ("||", "||"), ("and", "and"), ("or", "or")];
    for (k, v) in OPS {
        if *k == token {
            return Some(v);
        }
    }
    None
}

fn check_statement(kind: &str, state: &mut AnalysisState) {
    // Count statements, not every sub-expression. The old
    // `ends_with("_expression")` inflated counts ~3x (every `a + b`,
    // call arg, etc.), making `max_statements = 30` fail on ordinary code.
    if kind.ends_with("_statement")
        || kind.ends_with("_declaration")
        || kind.ends_with("_definition")
    {
        state.statements += 1;
    }
}

fn child_nesting(lang: SupportedLanguage, parent: &str, child: &str, depth: usize) -> usize {
    if (parent == "else_clause" && matches!(child, "if_expression" | "if_statement"))
        || (lang == SupportedLanguage::Python && child == "elif_clause")
    {
        depth.saturating_sub(1)
    } else {
        depth
    }
}
