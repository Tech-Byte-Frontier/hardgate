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
}

impl SupportedLanguage {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "rs" => Some(SupportedLanguage::Rust),
            "py" => Some(SupportedLanguage::Python),
            "ts" | "mts" | "cts" => Some(SupportedLanguage::TypeScript),
            "tsx" => Some(SupportedLanguage::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),
            _ => None,
        }
    }

    pub fn tree_sitter_language(&self) -> Language {
        match self {
            SupportedLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            SupportedLanguage::Python => tree_sitter_python::LANGUAGE.into(),
            SupportedLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            SupportedLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            SupportedLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        }
    }

    pub fn is_function_node(&self, kind: &str) -> bool {
        match self {
            SupportedLanguage::Rust => kind == "function_item",
            SupportedLanguage::Python => matches!(kind, "function_definition" | "lambda"),
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
        if crate::discovery::classification::is_retired_source(path) {
            anyhow::bail!(
                "unsupported analysis request for `{}`; Rust, Python and JavaScript/TypeScript analysis are supported",
                path.display()
            );
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = Self::from_extension(ext) else {
            return Ok(None);
        };
        let tree = lang
            .parse_tree(content)
            .ok_or_else(|| anyhow::anyhow!("Tree-sitter did not return a syntax tree"))?;
        if let Some(node) = first_syntax_error(tree.root_node(), lang, content.as_bytes()) {
            let point = node.start_position();
            let column = content
                .lines()
                .nth(point.row)
                .map_or(point.column + 1, |line| {
                    line.get(..point.column)
                        .map_or(point.column + 1, |prefix| prefix.chars().count() + 1)
                });
            let compiler = match lang {
                Self::Rust => "cargo check",
                Self::Python => "the project Python compiler",
                _ => "the project TypeScript/JavaScript compiler",
            };
            anyhow::bail!(
                "Hardgate parser could not analyze {}:{}:{} (syntax errors reported by Tree-sitter; source validity unconfirmed). Invalid source and unsupported parser syntax are distinct: verify with {compiler}. If the compiler accepts this file, this is a Hardgate parser limitation; report the syntax or use an equivalent imported type alias. No complete AST evidence was produced.",
                path.display(),
                point.row + 1,
                column
            );
        }
        Ok(Some((lang, tree)))
    }
}

fn first_syntax_error<'tree>(
    node: tree_sitter::Node<'tree>,
    lang: SupportedLanguage,
    source: &[u8],
) -> Option<tree_sitter::Node<'tree>> {
    if !node.has_error() {
        return None;
    }
    if (node.is_error() || node.is_missing()) && is_benign_jsx_attribute_error(node, lang, source) {
        return None;
    }
    // Prefer the smallest offending node over a broad recovery ERROR spanning the file.
    for index in 0..node.child_count() {
        if let Some(error) = node
            .child(index)
            .and_then(|child| first_syntax_error(child, lang, source))
        {
            return Some(error);
        }
    }
    (node.is_error() || node.is_missing()).then_some(node)
}

fn is_benign_jsx_attribute_error(
    node: tree_sitter::Node,
    lang: SupportedLanguage,
    source: &[u8],
) -> bool {
    if !matches!(lang, SupportedLanguage::Tsx | SupportedLanguage::JavaScript) {
        return false;
    }
    let Some(string) = node.parent().filter(|parent| parent.kind() == "string") else {
        return false;
    };
    // Only the grammar's bare ampersand error in a quoted attribute is benign.
    // Expressions such as label={a & & b} must still fail parsing.
    string
        .parent()
        .is_some_and(|parent| parent.kind() == "jsx_attribute")
        && !node.is_missing()
        && node
            .utf8_text(source)
            .is_ok_and(|text| text.starts_with('&'))
}
