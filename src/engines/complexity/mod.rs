pub mod languages;
mod size;
pub mod walker;
pub use size::SizeBreakdown;

use crate::config::FunctionBudgets;
pub use languages::SupportedLanguage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tree_sitter::Node;
pub use walker::ComplexityContribution;
use walker::{AnalysisState, WalkerContext, walk_visible_node};

/// Tree-sitter-derived metrics for one function: size, parameters, nesting,
/// cyclomatic scores, and per-node breakdowns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMetrics {
    pub name: String,
    pub file: PathBuf,
    pub start_line: usize,
    #[serde(default)]
    pub start_column: usize,
    pub end_line: usize,
    pub lines: usize,
    pub parameters: usize,
    pub cyclomatic: u32,
    pub max_nesting_depth: usize,
    pub statements: usize,
    #[serde(default)]
    pub test_only: bool,
    #[serde(default)]
    pub size: Option<SizeBreakdown>,
    pub cyclomatic_breakdown: Vec<ComplexityContribution>,
}

/// One function breaching a [`FunctionBudgets`] ceiling, with the top AST
/// contributors and a refactor recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityViolation {
    pub file: PathBuf,
    pub function_name: String,
    pub line_number: usize,
    #[serde(default)]
    pub column_number: usize,
    #[serde(default)]
    pub end_line: usize,
    pub metric: String,
    pub actual: f64,
    pub limit: f64,
    pub breakdown: Vec<ComplexityContribution>,
    pub message: String,
    pub recommendation: String,
    #[serde(default)]
    pub size: Option<SizeBreakdown>,
}

pub(crate) struct FileStructure {
    pub functions: Vec<FunctionMetrics>,
    pub size: Option<SizeBreakdown>,
}

/// Multi-language Tree-sitter analyzer producing [`FunctionMetrics`].
pub struct ComplexityAnalyzer;

struct ParseContext<'a> {
    source: &'a [u8],
    original: &'a [u8],
    lang: SupportedLanguage,
    file_path: &'a Path,
}

impl Default for ComplexityAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplexityAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Parse `content` and collect metrics for every function found.
    pub fn analyze_file(
        &mut self,
        path: &Path,
        content: &str,
        root: &Path,
    ) -> Vec<FunctionMetrics> {
        self.analyze_file_checked(path, content, root)
            .unwrap_or_default()
    }

    /// Checked variant used by strict gates so parser failures cannot turn
    /// into a zero-function success.
    pub fn analyze_file_checked(
        &mut self,
        path: &Path,
        content: &str,
        root: &Path,
    ) -> anyhow::Result<Vec<FunctionMetrics>> {
        self.analyze_file_structure(path, content, root)
            .map(|structure| structure.functions)
    }

    pub(crate) fn analyze_file_structure(
        &mut self,
        path: &Path,
        content: &str,
        root: &Path,
    ) -> anyhow::Result<FileStructure> {
        self.analyze_role_structure(path, (content, content), root)
    }

    /// Parse the actual source, then measure the byte-aligned ownership view.
    /// A projected field or statement need not form a standalone Rust file.
    pub(crate) fn analyze_role_structure(
        &mut self,
        path: &Path,
        content: (&str, &str),
        root: &Path,
    ) -> anyhow::Result<FileStructure> {
        let (original, visible) = content;
        let Some((lang, tree)) = SupportedLanguage::parse_file_checked(path, original)? else {
            return Ok(FileStructure {
                functions: Vec::new(),
                size: None,
            });
        };

        let rel_path = path.strip_prefix(root).unwrap_or(path);
        let ctx = ParseContext {
            source: visible.as_bytes(),
            original: original.as_bytes(),
            lang,
            file_path: rel_path,
        };

        let mut functions = Vec::new();
        collect_functions(tree.root_node(), &ctx, &mut functions);
        Ok(FileStructure {
            functions,
            size: Some(size::measure_file(tree.root_node(), visible.as_bytes())),
        })
    }

    /// Flag every metric in `metrics` that exceeds a `budgets` ceiling.
    pub fn check_violations(
        metrics: &[FunctionMetrics],
        budgets: &FunctionBudgets,
    ) -> Vec<ComplexityViolation> {
        let mut violations = Vec::new();
        for m in metrics {
            check_control_flow_limits(m, budgets, &mut violations);
            check_size_and_param_limits(m, budgets, &mut violations);
            check_statement_limit(m, budgets, &mut violations);
        }
        violations
    }
}

struct ViolationSpec<'a> {
    metric: &'a str,
    actual: f64,
    limit: f64,
    breakdown: &'a [ComplexityContribution],
    message: Option<String>,
    recommendation: String,
}

fn check_control_flow_limits(
    m: &FunctionMetrics,
    budgets: &FunctionBudgets,
    violations: &mut Vec<ComplexityViolation>,
) {
    if let Some(limit) = budgets.max_cyclomatic
        && m.cyclomatic > limit
    {
        violations.push(create_violation(
            m,
            ViolationSpec {
                metric: "Cyclomatic Complexity",
                actual: m.cyclomatic as f64,
                limit: limit as f64,
                breakdown: &m.cyclomatic_breakdown,
                message: None,
                recommendation: format!(
                    "Refactor `{}`: extract decision branches into helper functions.",
                    m.name
                ),
            },
        ));
    }
}

fn create_violation(m: &FunctionMetrics, spec: ViolationSpec) -> ComplexityViolation {
    let mut top = spec.breakdown.to_vec();
    top.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.line.cmp(&b.line)));
    ComplexityViolation {
        file: m.file.clone(),
        function_name: m.name.clone(),
        line_number: m.start_line,
        column_number: m.start_column,
        end_line: m.end_line,
        metric: spec.metric.to_string(),
        actual: spec.actual,
        limit: spec.limit,
        breakdown: top.into_iter().take(5).collect(),
        message: spec.message.unwrap_or_else(|| {
            format!(
                "{} is {:.0} (budget: {:.0})",
                spec.metric, spec.actual, spec.limit
            )
        }),
        recommendation: spec.recommendation,
        size: m.size.clone(),
    }
}

fn check_size_and_param_limits(
    m: &FunctionMetrics,
    budgets: &FunctionBudgets,
    violations: &mut Vec<ComplexityViolation>,
) {
    if let Some(limit) = budgets.max_parameters
        && m.parameters > limit
    {
        violations.push(create_violation(
            m,
            ViolationSpec {
                metric: "Parameter Count",
                actual: m.parameters as f64,
                limit: limit as f64,
                breakdown: &[],
                message: Some(format!(
                    "Function has {} parameters (budget: {})",
                    m.parameters, limit
                )),
                recommendation: format!(
                    "Introduce a config struct or parameter object for `{}`.",
                    m.name
                ),
            },
        ));
    }

    if let Some(limit) = budgets.max_lines
        && m.lines > limit
    {
        violations.push(create_violation(
            m,
            ViolationSpec {
                metric: "Function Lines",
                actual: m.lines as f64,
                limit: limit as f64,
                breakdown: &[],
                message: Some(format!(
                    "Function body spans {} lines (budget: {}){}",
                    m.lines, limit, m.size.as_ref().map(|size| format!("; {}", size.description())).unwrap_or_default()
                )),
                recommendation: format!("Review code and documentation in `{}` separately; extract cohesive code only where it improves clarity.", m.name),
            },
        ));
    }

    if let Some(limit) = budgets.max_nesting_depth
        && m.max_nesting_depth > limit
    {
        violations.push(create_violation(
            m,
            ViolationSpec {
                metric: "Nesting Depth",
                actual: m.max_nesting_depth as f64,
                limit: limit as f64,
                breakdown: &[],
                message: Some(format!(
                    "Max nesting depth is {} (budget: {})",
                    m.max_nesting_depth, limit
                )),
                recommendation: format!(
                    "Use early returns or guard clauses to reduce nesting depth in `{}`.",
                    m.name
                ),
            },
        ));
    }
}

fn check_statement_limit(
    m: &FunctionMetrics,
    budgets: &FunctionBudgets,
    violations: &mut Vec<ComplexityViolation>,
) {
    if let Some(limit) = budgets.max_statements
        && m.statements > limit
    {
        violations.push(create_violation(
            m,
            ViolationSpec {
                metric: "Statement Count",
                actual: m.statements as f64,
                limit: limit as f64,
                breakdown: &[],
                message: Some(format!(
                    "Function has {} statements (budget: {})",
                    m.statements, limit
                )),
                recommendation: format!("Split `{}` into smaller focused functions.", m.name),
            },
        ));
    }
}

fn collect_functions(node: Node, ctx: &ParseContext, results: &mut Vec<FunctionMetrics>) {
    if ctx.lang.is_function_node(node.kind())
        && let Some(metrics) = analyze_function_node(node, ctx)
    {
        results.push(metrics);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_functions(child, ctx, results);
    }
}

fn analyze_function_node(node: Node, ctx: &ParseContext) -> Option<FunctionMetrics> {
    if ctx.source[node.start_byte()].is_ascii_whitespace() {
        return None;
    }
    let name = extract_function_name(node, ctx.source, ctx.lang)?;
    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let size = size::measure_projection(node, ctx.source, ctx.original);
    let lines = size.physical_lines;
    let parameters = count_parameters(node, ctx.lang, ctx.source);

    let walker_ctx = WalkerContext { lang: ctx.lang };

    let mut state = AnalysisState::new();
    walk_visible_node(node, (&walker_ctx, ctx.source), 0, &mut state);

    Some(FunctionMetrics {
        name,
        file: ctx.file_path.to_path_buf(),
        start_line,
        start_column: node.start_position().column + 1,
        end_line,
        lines,
        parameters,
        cyclomatic: state.cyclomatic,
        max_nesting_depth: state.max_nesting_depth,
        statements: state.statements,
        test_only: false,
        size: Some(size),
        cyclomatic_breakdown: state.cyclomatic_breakdown,
    })
}

fn extract_function_name(node: Node, source: &[u8], lang: SupportedLanguage) -> Option<String> {
    if (lang == SupportedLanguage::TypeScript
        || lang == SupportedLanguage::Tsx
        || lang == SupportedLanguage::JavaScript)
        && node.kind() == "arrow_function"
    {
        return Some(
            extract_declarator_name(node, source).unwrap_or_else(|| "anonymous".to_string()),
        );
    }

    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        if is_name_identifier(child.kind()) {
            return child.utf8_text(source).ok().map(|s| s.to_string());
        }
    }

    if (lang == SupportedLanguage::TypeScript
        || lang == SupportedLanguage::Tsx
        || lang == SupportedLanguage::JavaScript)
        && let Some(arrow_name) = extract_declarator_name(node, source)
    {
        return Some(arrow_name);
    }

    Some("anonymous".to_string())
}

fn is_name_identifier(kind: &str) -> bool {
    kind == "identifier" || kind == "property_identifier" || kind == "field_identifier"
}

fn extract_declarator_name(node: Node, source: &[u8]) -> Option<String> {
    let parent = node.parent()?;
    if parent.kind() == "variable_declarator" {
        let id_node = parent.child_by_field_name("name")?;
        return id_node.utf8_text(source).ok().map(|s| s.to_string());
    }
    None
}

fn count_parameters(node: Node, lang: SupportedLanguage, source: &[u8]) -> usize {
    let param_kind = match lang {
        SupportedLanguage::Rust => "parameters",
        SupportedLanguage::TypeScript | SupportedLanguage::Tsx | SupportedLanguage::JavaScript => {
            "formal_parameters"
        }
    };

    let Some(child) = (0..node.child_count()).find_map(|i| {
        let c = node.child(i)?;
        if c.kind() == param_kind {
            Some(c)
        } else {
            None
        }
    }) else {
        return 0;
    };

    (0..child.child_count())
        .filter_map(|j| child.child(j))
        .filter(|param| {
            let kind = param.kind();
            !source[param.start_byte()].is_ascii_whitespace()
                && !matches!(
                    kind,
                    "(" | ")" | "," | "{" | "}" | "[" | "]" | "*" | "/" | ":"
                )
                && !kind.contains("comment")
        })
        .count()
}
