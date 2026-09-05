use crate::engines::complexity::SupportedLanguage;
use serde::{Deserialize, Serialize};
use std::io;
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
        self.generate_with_limit(path, content, None).candidates
    }

    pub(crate) fn generate_mutants_bounded(
        &mut self,
        path: &Path,
        content: &str,
        maximum: usize,
    ) -> io::Result<Vec<AstMutant>> {
        let generated = self.generate_with_limit(path, content, Some(maximum));
        if generated.exhausted {
            return Err(io::Error::other(format!(
                "mutation resource guard: mutant candidate limit exceeded: maximum {maximum} candidates"
            )));
        }
        Ok(generated.candidates)
    }

    fn generate_with_limit(
        &mut self,
        path: &Path,
        content: &str,
        maximum: Option<usize>,
    ) -> GenerationResult {
        let Some((_lang, tree)) = SupportedLanguage::parse_file(path, content) else {
            return GenerationResult::default();
        };

        let mut generated = GenerationResult {
            maximum,
            ..GenerationResult::default()
        };
        collect_ast_mutants(tree.root_node(), content.as_bytes(), path, &mut generated);
        generated
    }
}

#[derive(Default)]
struct GenerationResult {
    candidates: Vec<AstMutant>,
    exhausted: bool,
    maximum: Option<usize>,
}

fn collect_ast_mutants(root: Node, source: &[u8], path: &Path, generated: &mut GenerationResult) {
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if !collect_node_mutants(node, source, path, generated) {
            return;
        }

        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

fn collect_node_mutants(
    node: Node,
    source: &[u8],
    path: &Path,
    generated: &mut GenerationResult,
) -> bool {
    if node.kind() == "binary_expression" {
        collect_binary_mutants(node, source, path, generated)
    } else if let Some(mutant) =
        try_mutate_boolean(node, source, path, generated.candidates.len() + 1)
    {
        push_candidate(generated, mutant)
    } else {
        true
    }
}

fn collect_binary_mutants(
    node: Node,
    source: &[u8],
    path: &Path,
    generated: &mut GenerationResult,
) -> bool {
    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        let Ok(op_text) = child.utf8_text(source) else {
            continue;
        };
        if let Some(rep) = invert_binary_op(op_text) {
            let id = generated.candidates.len() + 1;
            let line = child.start_position().row + 1;
            let column = child.start_position().column + 1;
            let mutant = AstMutant {
                id,
                file: path.to_path_buf(),
                line,
                column,
                start_byte: child.start_byte(),
                end_byte: child.end_byte(),
                original: op_text.to_string(),
                replacement: rep.to_string(),
                description: format!("Replace `{}` with `{}`", op_text, rep),
            };
            if !push_candidate(generated, mutant) {
                return false;
            }
        }
    }
    true
}

fn push_candidate(generated: &mut GenerationResult, candidate: AstMutant) -> bool {
    if generated
        .maximum
        .is_some_and(|maximum| generated.candidates.len() >= maximum)
    {
        generated.exhausted = true;
        return false;
    }
    generated.candidates.push(candidate);
    true
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

#[cfg(test)]
#[path = "generator_tests.rs"]
mod tests;
