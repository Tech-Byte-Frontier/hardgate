//! Python import spellings come from the grammar, including aliases and
//! parenthesized imports. This does not resolve packages or re-exports.
use super::{CheckContext, CompiledInvariantRule, InvariantViolation, create_violation};
use crate::engines::complexity::SupportedLanguage;
use std::path::Path;
use tree_sitter::Node;

pub(super) fn check(
    file: (&Path, &str),
    rule: &CompiledInvariantRule,
    violations: &mut Vec<InvariantViolation>,
) {
    if file.0.extension().is_none_or(|extension| extension != "py") {
        return;
    }
    let Some(globs) = &rule.disallow_imports else {
        return;
    };
    let Some((_, tree)) = SupportedLanguage::parse_file(file.0, file.1) else {
        return;
    };
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        if matches!(node.kind(), "import_statement" | "import_from_statement") {
            let line = node.start_position().row + 1;
            let context = CheckContext {
                rel_path: file.0,
                line_number: line,
                line: file.1.lines().nth(line - 1).unwrap_or(""),
                rule,
            };
            for target in targets(node, file.1.as_bytes()) {
                if globs.is_match(&target) {
                    violations.push(create_violation(&context, "Disallowed Import", &target));
                }
            }
        } else {
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
    }
}

fn targets(node: Node<'_>, source: &[u8]) -> Vec<String> {
    let module = node
        .child_by_field_name("module_name")
        .and_then(|node| node.utf8_text(source).ok());
    let mut targets = module.map_or_else(Vec::new, |name| vec![name.into()]);
    let mut cursor = node.walk();
    for name in node.children_by_field_name("name", &mut cursor) {
        let name = name.child_by_field_name("name").unwrap_or(name);
        if let Ok(name) = name.utf8_text(source) {
            targets.push(match module {
                Some(module) if module.ends_with('.') => format!("{module}{name}"),
                Some(module) => format!("{module}.{name}"),
                None => name.into(),
            });
        }
    }
    targets
}
