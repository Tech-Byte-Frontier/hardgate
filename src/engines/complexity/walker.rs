use super::languages::SupportedLanguage;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    pub cognitive: u32,
    pub max_nesting_depth: usize,
    pub statements: usize,
    pub assignments: usize,
    pub branches: usize,
    pub calls: usize,
    pub operators: HashSet<String>,
    pub operands: HashSet<String>,
    pub total_operators: usize,
    pub total_operands: usize,
    pub cognitive_breakdown: Vec<ComplexityContribution>,
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

        let cogn_score = 1 + current_nesting as u32;
        state.cognitive += cogn_score;
        state.cognitive_breakdown.push(ComplexityContribution {
            line,
            column,
            kind: kind.to_string(),
            description: if current_nesting > 0 {
                format!("{} (nesting level {})", branch_desc, current_nesting)
            } else {
                branch_desc.to_string()
            },
            score: cogn_score,
        });
    }

    check_boolean_operator(node, ctx.source, kind, state);
    check_statement(kind, state);
    check_halstead(node, ctx.source, kind, state);
    check_abc(kind, state);

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
            | "elif_clause"
            | "else_if_clause"
            | "while_expression"
            | "while_statement"
            | "for_expression"
            | "for_statement"
            | "for_in_statement"
            | "for_clause"
            | "loop_expression"
            | "match_expression"
            | "match_arm"
            | "switch_statement"
            | "expression_switch_statement"
            | "switch_case"
            | "expression_case"
            | "catch_clause"
            | "except_clause"
            | "exception_handler"
            | "ternary_expression"
            | "conditional_expression"
    )
}

fn human_readable_branch(kind: &str) -> &'static str {
    // Table lookup keeps cyclomatic low despite many node kinds.
    const TABLE: &[(&[&str], &str)] = &[
        (
            &["if_expression", "if_statement"],
            "conditional branch (`if`)",
        ),
        (
            &["elif_clause", "else_if_clause"],
            "conditional branch (`elif`)",
        ),
        (&["while_expression", "while_statement"], "loop (`while`)"),
        (
            &[
                "for_expression",
                "for_statement",
                "for_in_statement",
                "for_clause",
            ],
            "loop (`for`)",
        ),
        (&["loop_expression"], "infinite loop (`loop`)"),
        (&["match_expression"], "pattern match (`match`)"),
        (&["match_arm"], "pattern match arm (`match`)"),
        (
            &["switch_statement", "expression_switch_statement"],
            "switch (`switch`)",
        ),
        (&["switch_case", "expression_case"], "switch case"),
        (
            &["catch_clause", "except_clause", "exception_handler"],
            "exception handler (`catch`)",
        ),
        (&["ternary_expression"], "ternary operator (`? :`)"),
        (
            &["conditional_expression"],
            "conditional expression (`value if condition else fallback`)",
        ),
    ];
    for (kinds, desc) in TABLE {
        if kinds.contains(&kind) {
            return desc;
        }
    }
    "branching construct"
}

fn check_boolean_operator(node: Node, source: &[u8], kind: &str, state: &mut AnalysisState) {
    if kind != "binary_expression" && kind != "boolean_operator" {
        return;
    }
    // Prefer the direct operator token over whole-subtree text so nested
    // `a && b || c` counts each operator once instead of double-counting
    // the outer node (whose text contains both operators).
    let op_label = direct_boolean_operator(node, source).or_else(|| {
        let Ok(text) = node.utf8_text(source) else {
            return None;
        };
        if text.contains("&&") {
            Some("&&")
        } else if text.contains("||") {
            Some("||")
        } else if text.contains(" and ") {
            Some("and")
        } else if text.contains(" or ") {
            Some("or")
        } else {
            None
        }
    });

    if let Some(op) = op_label {
        let line = node.start_position().row + 1;
        let column = node.start_position().column + 1;
        let desc = format!("boolean operator `{}`", op);

        state.cyclomatic += 1;
        state.cyclomatic_breakdown.push(ComplexityContribution {
            line,
            column,
            kind: "boolean_operator".to_string(),
            description: desc.clone(),
            score: 1,
        });

        state.cognitive += 1;
        state.cognitive_breakdown.push(ComplexityContribution {
            line,
            column,
            kind: "boolean_operator".to_string(),
            description: desc,
            score: 1,
        });
    }
}

fn direct_boolean_operator(node: Node, source: &[u8]) -> Option<&'static str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(op) = classify_operator_token(child.kind()) {
            return Some(op);
        }
        if child.child_count() == 0
            && let Ok(t) = child.utf8_text(source)
            && let Some(op) = classify_operator_token(t)
        {
            return Some(op);
        }
    }
    None
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

fn check_abc(kind: &str, state: &mut AnalysisState) {
    // ABC metric: Assignments, Branches, Calls.
    match kind {
        "assignment_expression" | "augmented_assignment_expression" | "assignment" => {
            state.assignments += 1;
        }
        "call_expression" | "call" | "method_call" => {
            state.calls += 1;
        }
        _ => {}
    }
    if check_branch(kind) {
        state.branches += 1;
    }
}

pub fn abc_score(assignments: usize, branches: usize, calls: usize) -> f64 {
    ((assignments * assignments + branches * branches + calls * calls) as f64).sqrt()
}

fn check_halstead(node: Node, source: &[u8], kind: &str, state: &mut AnalysisState) {
    if node.child_count() > 0 {
        return;
    }
    let Ok(text) = node.utf8_text(source) else {
        return;
    };
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
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "="
            | "=="
            | "!="
            | "<"
            | "<="
            | ">"
            | ">="
            | "&&"
            | "||"
            | "!"
            | "&"
            | "|"
            | "^"
            | "<<"
            | ">>"
            | "+="
            | "-="
            | "*="
            | "/="
            | "fn"
            | "let"
            | "mut"
            | "if"
            | "else"
            | "match"
            | "switch"
            | "case"
            | "for"
            | "while"
            | "return"
            | "break"
            | "continue"
            | "try"
            | "catch"
            | "def"
            | "func"
            | "const"
            | "var"
            | "function"
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
