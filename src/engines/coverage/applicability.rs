use crate::engines::complexity::languages::SupportedLanguage;
use std::path::Path;
use tree_sitter::Node;

/// Execution producers cannot instrument stylesheets or erased TypeScript
/// declarations. Unknown languages and unreadable/rejected syntax remain
/// required: absence of a parser is never proof of absent executable code.
pub(crate) fn execution_not_applicable(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if extension.eq_ignore_ascii_case("css") {
        return true;
    }
    if !matches!(
        SupportedLanguage::from_extension(extension),
        Some(SupportedLanguage::TypeScript | SupportedLanguage::Tsx)
    ) {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(Some((_, tree))) = SupportedLanguage::parse_file_checked(path, &content) else {
        return false;
    };
    tree.root_node()
        .named_children(&mut tree.root_node().walk())
        .all(erased_declaration)
}

fn erased_declaration(node: Node<'_>) -> bool {
    match node.kind() {
        "comment"
        | "empty_statement"
        | "interface_declaration"
        | "type_alias_declaration"
        | "ambient_declaration" => true,
        "import_statement" => has_type_modifier(node),
        "export_statement" => {
            has_type_modifier(node)
                || node
                    .child_by_field_name("declaration")
                    .is_some_and(erased_declaration)
        }
        _ => false,
    }
}

fn has_type_modifier(node: Node<'_>) -> bool {
    node.children(&mut node.walk())
        .any(|child| !child.is_named() && child.kind() == "type")
}

#[cfg(test)]
#[path = "applicability_tests.rs"]
mod tests;
