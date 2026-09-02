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
            SupportedLanguage::TypeScript | SupportedLanguage::Tsx | SupportedLanguage::JavaScript => {
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
}
