use tree_sitter::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        match ext {
            "rs" => Some(SupportedLanguage::Rust),
            "ts" => Some(SupportedLanguage::TypeScript),
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
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&self.tree_sitter_language()).ok()?;
        parser.parse(content, None)
    }

    pub fn parse_file(path: &std::path::Path, content: &str) -> Option<(Self, tree_sitter::Tree)> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = Self::from_extension(ext)?;
        let tree = lang.parse_tree(content)?;
        Some((lang, tree))
    }
}
