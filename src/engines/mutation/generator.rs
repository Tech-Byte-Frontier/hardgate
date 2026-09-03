use crate::engines::complexity::SupportedLanguage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tree_sitter::Node;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstMutant {
    pub id: usize,
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub original: String,
    pub replacement: String,
    pub description: String,
}

pub struct AstMutationGenerator;

impl Default for AstMutationGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl AstMutationGenerator {
    pub fn new() -> Self {
        Self
    }

    pub fn generate_mutants(&mut self, path: &Path, content: &str) -> Vec<AstMutant> {
        let Some((_lang, tree)) = SupportedLanguage::parse_file(path, content) else {
            return Vec::new();
        };

        let mut mutants = Vec::new();
        collect_ast_mutants(tree.root_node(), content.as_bytes(), path, &mut mutants);
        mutants
    }
}

fn collect_ast_mutants(node: Node, source: &[u8], path: &Path, mutants: &mut Vec<AstMutant>) {
    if node.kind() == "binary_expression" {
        collect_binary_mutants(node, source, path, mutants);
    } else if let Some(m) = try_mutate_boolean(node, source, path, mutants.len() + 1) {
        mutants.push(m);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ast_mutants(child, source, path, mutants);
    }
}

fn collect_binary_mutants(node: Node, source: &[u8], path: &Path, mutants: &mut Vec<AstMutant>) {
    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        let Ok(op_text) = child.utf8_text(source) else {
            continue;
        };
        if let Some(rep) = invert_binary_op(op_text) {
            let id = mutants.len() + 1;
            let line = child.start_position().row + 1;
            let column = child.start_position().column + 1;
            mutants.push(AstMutant {
                id,
                file: path.to_path_buf(),
                line,
                column,
                start_byte: child.start_byte(),
                end_byte: child.end_byte(),
                original: op_text.to_string(),
                replacement: rep.to_string(),
                description: format!("Replace `{}` with `{}`", op_text, rep),
            });
        }
    }
}

const BINARY_MUTATIONS: &[(&str, &str)] = &[
    ("==", "!="),
    ("!=", "=="),
    ("<", ">="),
    ("<=", ">"),
    (">", "<="),
    (">=", "<"),
    ("&&", "||"),
    ("||", "&&"),
    ("+", "-"),
    ("-", "+"),
    ("*", "/"),
    ("/", "*"),
];

fn invert_binary_op(op: &str) -> Option<&'static str> {
    for &(original, mutated) in BINARY_MUTATIONS {
        if original == op {
            return Some(mutated);
        }
    }
    None
}

fn try_mutate_boolean(node: Node, source: &[u8], path: &Path, id: usize) -> Option<AstMutant> {
    let kind = node.kind();
    if kind != "boolean_literal" && kind != "true" && kind != "false" {
        return None;
    }
    let text = node.utf8_text(source).ok()?;
    let replacement = match text {
        "true" => "false",
        "false" => "true",
        _ => return None,
    };
    Some(AstMutant {
        id,
        file: path.to_path_buf(),
        line: node.start_position().row + 1,
        column: node.start_position().column + 1,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        original: text.to_string(),
        replacement: replacement.to_string(),
        description: format!("Replace boolean `{}` with `{}`", text, replacement),
    })
}
