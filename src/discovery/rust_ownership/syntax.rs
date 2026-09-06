use super::cfg;
use std::ops::Range;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Tree};

pub(super) struct Module {
    pub candidates: Vec<PathBuf>,
    pub test_only: bool,
}

#[derive(Default)]
pub(super) struct Syntax {
    pub test_ranges: Vec<Range<usize>>,
    pub modules: Vec<Module>,
    pub file_test: bool,
    pub uncertain_modules: bool,
}

struct Context<'a> {
    source: &'a [u8],
    module_dir: PathBuf,
    attribute_dir: PathBuf,
    testing: bool,
}

pub(super) fn analyze(tree: &Tree, source: &str, path: &Path, crate_root: bool) -> Syntax {
    let parent = path.parent().unwrap_or(Path::new(""));
    let module_dir = if crate_root || path.file_name().is_some_and(|name| name == "mod.rs") {
        parent.to_path_buf()
    } else {
        parent.join(path.file_stem().unwrap_or_default())
    };
    let mut result = Syntax {
        file_test: inner_test(tree.root_node(), source.as_bytes()),
        ..Default::default()
    };
    if result.file_test {
        result.test_ranges.push(0..source.len());
    }
    walk(
        tree.root_node(),
        &Context {
            source: source.as_bytes(),
            module_dir,
            attribute_dir: parent.to_path_buf(),
            testing: result.file_test,
        },
        &mut result,
    );
    result
        .test_ranges
        .sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
    let mut end = 0;
    result.test_ranges.retain(|range| {
        if range.end <= end {
            false
        } else {
            end = range.end;
            true
        }
    });
    result
}

fn inner_test(node: Node<'_>, source: &[u8]) -> bool {
    node.named_children(&mut node.walk())
        .any(|child| child.kind() == "inner_attribute_item" && cfg::requires_test(child, source))
}

fn walk(node: Node<'_>, context: &Context<'_>, result: &mut Syntax) {
    // These grammar nodes own their attributes internally. Treating the
    // attribute as a sibling of just the field name would leave its value in
    // production (and lose the ownership of branches inside that value).
    let nested;
    let context = if !context.testing && owns_test_attribute(node, context.source) {
        result.test_ranges.push(node.start_byte()..owned_end(node));
        nested = Context {
            source: context.source,
            module_dir: context.module_dir.clone(),
            attribute_dir: context.attribute_dir.clone(),
            testing: true,
        };
        &nested
    } else {
        context
    };
    if skip_tokens(node, context, result) {
        return;
    }
    let mut attributes = Attributes::default();
    for child in node.named_children(&mut node.walk()) {
        if attributes.consume(child, context.source) {
            continue;
        }
        let test_only = context.testing
            || attributes
                .items
                .iter()
                .any(|attr| cfg::requires_test(*attr, context.source));
        if test_only && !context.testing {
            let start = attributes
                .doc_start
                .or_else(|| attributes.items.first().map(Node::start_byte))
                .unwrap_or(child.start_byte());
            result.test_ranges.push(start..owned_end(child));
        }
        let child_context = Context {
            testing: test_only,
            module_dir: context.module_dir.clone(),
            attribute_dir: context.attribute_dir.clone(),
            source: context.source,
        };
        if child.kind() == "mod_item" {
            module(child, &attributes.items, &child_context, result);
        } else {
            walk(child, &child_context, result);
        }
        attributes = Attributes::default();
    }
}

fn skip_tokens(node: Node<'_>, context: &Context<'_>, result: &mut Syntax) -> bool {
    // Tokens in a macro body do not declare modules in the enclosing module.
    if matches!(node.kind(), "token_tree" | "token_tree_pattern") {
        result.uncertain_modules |= !context.testing && has_module_token(node);
        return true;
    }
    if !context.testing
        && node.kind() == "macro_invocation"
        && node
            .child_by_field_name("macro")
            .and_then(|name| name.utf8_text(context.source).ok())
            .is_some_and(|name| name == "include")
    {
        result.uncertain_modules = true;
    }
    false
}

#[derive(Default)]
struct Attributes<'a> {
    items: Vec<Node<'a>>,
    doc_start: Option<usize>,
}

impl<'a> Attributes<'a> {
    fn consume(&mut self, child: Node<'a>, source: &[u8]) -> bool {
        match child.kind() {
            "attribute_item" => self.items.push(child),
            "inner_attribute_item" => {}
            "line_comment" | "block_comment" => {
                if child
                    .utf8_text(source)
                    .is_ok_and(|text| text.starts_with("///") || text.starts_with("/**"))
                {
                    self.doc_start.get_or_insert(child.start_byte());
                }
            }
            _ => return false,
        }
        true
    }
}

fn owns_test_attribute(node: Node<'_>, source: &[u8]) -> bool {
    matches!(
        node.kind(),
        "field_initializer"
            | "shorthand_field_initializer"
            | "match_arm"
            | "parameter"
            | "self_parameter"
            | "enum_variant"
    ) && node
        .named_children(&mut node.walk())
        .any(|child| child.kind() == "attribute_item" && cfg::requires_test(child, source))
}

fn owned_end(node: Node<'_>) -> usize {
    node.next_sibling()
        .filter(|next| matches!(next.kind(), "," | ";"))
        .map_or(node.end_byte(), |next| next.end_byte())
}

fn module(node: Node<'_>, attributes: &[Node<'_>], context: &Context<'_>, result: &mut Syntax) {
    let Some(name) = node
        .child_by_field_name("name")
        .and_then(|name| name.utf8_text(context.source).ok())
    else {
        return;
    };
    let paths = attributes
        .iter()
        .filter_map(|attr| cfg::path_attribute(*attr, context.source))
        .collect::<Vec<_>>();
    // Conditional path rewriting is not a proof of exclusive test ownership.
    if paths.len() > 1
        || attributes.iter().any(|attr| {
            attr.utf8_text(context.source)
                .is_ok_and(|text| text.contains("cfg_attr") && text.contains("path"))
        })
    {
        return;
    }
    let directory = paths.first().map_or_else(
        || context.module_dir.join(name),
        |path| context.attribute_dir.join(path),
    );
    if let Some(body) = node.child_by_field_name("body") {
        let testing = context.testing || inner_test(body, context.source);
        if testing && !context.testing {
            result.test_ranges.push(node.byte_range());
        }
        walk(
            body,
            &Context {
                source: context.source,
                attribute_dir: directory.clone(),
                module_dir: directory,
                testing,
            },
            result,
        );
    } else {
        let candidates = if let Some(path) = paths.first() {
            vec![context.attribute_dir.join(path)]
        } else {
            vec![directory.with_extension("rs"), directory.join("mod.rs")]
        };
        result.modules.push(Module {
            candidates,
            test_only: context.testing,
        });
    }
}

fn has_module_token(node: Node<'_>) -> bool {
    node.kind() == "mod" || node.children(&mut node.walk()).any(has_module_token)
}
