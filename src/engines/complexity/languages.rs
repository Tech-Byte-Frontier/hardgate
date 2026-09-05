use std::cell::RefCell;
use std::collections::{HashMap, hash_map::Entry};
use tree_sitter::Language;

thread_local! {
    static PARSERS: RefCell<HashMap<SupportedLanguage, tree_sitter::Parser>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedLanguage {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
}

impl SupportedLanguage {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "rs" => Some(SupportedLanguage::Rust),
            "ts" | "mts" | "cts" => Some(SupportedLanguage::TypeScript),
            "tsx" => Some(SupportedLanguage::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),
            "py" => Some(SupportedLanguage::Python),
            "go" => Some(SupportedLanguage::Go),
            _ => None,
        }
    }

    pub fn tree_sitter_language(&self) -> Language {
        match self {
            SupportedLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            SupportedLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            SupportedLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            SupportedLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            SupportedLanguage::Python => tree_sitter_python::LANGUAGE.into(),
            SupportedLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    pub fn is_function_node(&self, kind: &str) -> bool {
        match self {
            SupportedLanguage::Rust => kind == "function_item",
            SupportedLanguage::TypeScript
            | SupportedLanguage::Tsx
            | SupportedLanguage::JavaScript => {
                matches!(
                    kind,
                    "function_declaration"
                        | "method_definition"
                        | "arrow_function"
                        | "function_expression"
                )
            }
            SupportedLanguage::Python => kind == "function_definition",
            SupportedLanguage::Go => matches!(kind, "function_declaration" | "method_declaration"),
        }
    }

    pub fn parse_tree(&self, content: &str) -> Option<tree_sitter::Tree> {
        // Each analysis worker owns at most one parser per supported language.
        // Trees remain owned outputs; source text and prior trees are not cached.
        PARSERS.with(|parsers| {
            let mut parsers = parsers.borrow_mut();
            let parser = match parsers.entry(*self) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let mut parser = tree_sitter::Parser::new();
                    parser.set_language(&self.tree_sitter_language()).ok()?;
                    entry.insert(parser)
                }
            };
            parser.parse(content, None)
        })
    }

    pub fn parse_file(path: &std::path::Path, content: &str) -> Option<(Self, tree_sitter::Tree)> {
        Self::parse_file_checked(path, content).ok().flatten()
    }

    /// Parse a supported source file and fail when Tree-sitter reports syntax
    /// errors. Unsupported extensions return `Ok(None)` so classification can
    /// decide whether that absence is permitted by policy.
    pub fn parse_file_checked(
        path: &std::path::Path,
        content: &str,
    ) -> anyhow::Result<Option<(Self, tree_sitter::Tree)>> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = Self::from_extension(ext) else {
            return Ok(None);
        };
        let tree = lang
            .parse_tree(content)
            .ok_or_else(|| anyhow::anyhow!("Tree-sitter did not return a syntax tree"))?;
        if has_syntax_errors(&tree, lang, content.as_bytes()) {
            anyhow::bail!("Tree-sitter found syntax errors in {}", path.display());
        }
        Ok(Some((lang, tree)))
    }
}

fn has_syntax_errors(tree: &tree_sitter::Tree, lang: SupportedLanguage, source: &[u8]) -> bool {
    let root = tree.root_node();
    if !root.has_error() {
        return false;
    }
    has_genuine_syntax_error(root, lang, source)
}

fn has_genuine_syntax_error(node: tree_sitter::Node, lang: SupportedLanguage, source: &[u8]) -> bool {
    if node.is_error() || node.is_missing() {
        if is_benign_jsx_attribute_error(node, lang, source) {
            return false;
        }
        return true;
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i)
            && child.has_error()
            && has_genuine_syntax_error(child, lang, source)
        {
            return true;
        }
    }
    false
}

fn is_benign_jsx_attribute_error(
    node: tree_sitter::Node,
    lang: SupportedLanguage,
    source: &[u8],
) -> bool {
    if !matches!(lang, SupportedLanguage::Tsx | SupportedLanguage::JavaScript) {
        return false;
    }
    let mut current = Some(node);
    let mut in_jsx_attribute = false;
    while let Some(parent) = current.and_then(|n| n.parent()) {
        if parent.kind() == "jsx_attribute" {
            in_jsx_attribute = true;
            break;
        }
        current = Some(parent);
    }
    if !in_jsx_attribute {
        return false;
    }
    let text = node.utf8_text(source).unwrap_or_default();
    text.contains('&')
}

