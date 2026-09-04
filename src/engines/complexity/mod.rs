pub mod languages;
pub mod walker;

use crate::config::FunctionBudgets;
pub use languages::SupportedLanguage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tree_sitter::Node;
pub use walker::ComplexityContribution;
use walker::{AnalysisState, WalkerContext, abc_score, walk_node};

/// Tree-sitter-derived metrics for one function: size, parameters, nesting,
/// cyclomatic/cognitive/Halstead/ABC scores, and per-node breakdowns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMetrics {
    pub name: String,
    pub file: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub lines: usize,
    pub parameters: usize,
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub halstead_difficulty: f64,
    pub max_nesting_depth: usize,
    pub statements: usize,
    #[serde(default)]
    pub abc_score: f64,
    pub cognitive_breakdown: Vec<ComplexityContribution>,
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
    pub end_line: usize,
    pub metric: String,
    pub actual: f64,
    pub limit: f64,
    pub breakdown: Vec<ComplexityContribution>,
    pub message: String,
    pub recommendation: String,
}

/// Multi-language Tree-sitter analyzer producing [`FunctionMetrics`].
pub struct ComplexityAnalyzer;

struct ParseContext<'a> {
    source: &'a [u8],
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
        let Some((lang, tree)) = SupportedLanguage::parse_file_checked(path, content)? else {
            return Ok(Vec::new());
        };

        let rel_path = path.strip_prefix(root).unwrap_or(path);
        let ctx = ParseContext {
            source: content.as_bytes(),
            lang,
            file_path: rel_path,
        };

        let mut functions = Vec::new();
        collect_functions(tree.root_node(), &ctx, &mut functions);
        Ok(functions)
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
            check_advanced_limits(m, budgets, &mut violations);
        }
        violations
    }
}

struct ViolationSpec<'a> {
    metric: &'a str,
    actual: f64,
    limit: f64,
    breakdown: &'a [ComplexityContribution],
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
        violations.push(create_complexity_violation(
            m,
            ViolationSpec {
                metric: "Cyclomatic Complexity",
                actual: m.cyclomatic as f64,
                limit: limit as f64,
                breakdown: &m.cyclomatic_breakdown,
                recommendation: format!(
                    "Refactor `{}`: extract decision branches into helper functions.",
                    m.name
                ),
            },
        ));
    }

    if let Some(limit) = budgets.max_cognitive
        && m.cognitive > limit
    {
        violations.push(create_complexity_violation(
            m,
            ViolationSpec {
                metric: "Cognitive Complexity",
                actual: m.cognitive as f64,
                limit: limit as f64,
                breakdown: &m.cognitive_breakdown,
                recommendation: format!("Flatten nested control structures in `{}`.", m.name),
            },
        ));
    }
}

fn create_complexity_violation(m: &FunctionMetrics, spec: ViolationSpec) -> ComplexityViolation {
    let mut top = spec.breakdown.to_vec();
    top.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.line.cmp(&b.line)));
    ComplexityViolation {
        file: m.file.clone(),
        function_name: m.name.clone(),
        line_number: m.start_line,
        end_line: m.end_line,
        metric: spec.metric.to_string(),
        actual: spec.actual,
        limit: spec.limit,
        breakdown: top.into_iter().take(5).collect(),
        message: format!(
            "{} is {:.0} (budget: {:.0})",
            spec.metric, spec.actual, spec.limit
        ),
        recommendation: spec.recommendation,
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
        violations.push(ComplexityViolation {
            file: m.file.clone(),
            function_name: m.name.clone(),
            line_number: m.start_line,
            end_line: m.end_line,
            metric: "Parameter Count".to_string(),
            actual: m.parameters as f64,
            limit: limit as f64,
            breakdown: Vec::new(),
            message: format!(
                "Function has {} parameters (budget: {})",
                m.parameters, limit
            ),
            recommendation: format!(
                "Introduce a config struct or parameter object for `{}`.",
                m.name
            ),
        });
    }

    if let Some(limit) = budgets.max_lines
        && m.lines > limit
    {
        violations.push(ComplexityViolation {
            file: m.file.clone(),
            function_name: m.name.clone(),
            line_number: m.start_line,
            end_line: m.end_line,
            metric: "Function Lines".to_string(),
            actual: m.lines as f64,
            limit: limit as f64,
            breakdown: Vec::new(),
            message: format!("Function body spans {} lines (budget: {})", m.lines, limit),
            recommendation: format!("Split `{}` into smaller focused functions.", m.name),
        });
    }

    if let Some(limit) = budgets.max_nesting_depth
        && m.max_nesting_depth > limit
    {
        violations.push(ComplexityViolation {
            file: m.file.clone(),
            function_name: m.name.clone(),
            line_number: m.start_line,
            end_line: m.end_line,
            metric: "Nesting Depth".to_string(),
            actual: m.max_nesting_depth as f64,
            limit: limit as f64,
            breakdown: Vec::new(),
            message: format!(
                "Max nesting depth is {} (budget: {})",
                m.max_nesting_depth, limit
            ),
            recommendation: format!(
                "Use early returns or guard clauses to reduce nesting depth in `{}`.",
                m.name
            ),
        });
    }
}

fn check_advanced_limits(
    m: &FunctionMetrics,
    budgets: &FunctionBudgets,
    violations: &mut Vec<ComplexityViolation>,
) {
    if let Some(limit) = budgets.max_halstead_difficulty
        && m.halstead_difficulty > limit
    {
        violations.push(ComplexityViolation {
            file: m.file.clone(),
            function_name: m.name.clone(),
            line_number: m.start_line,
            end_line: m.end_line,
            metric: "Halstead Difficulty".to_string(),
            actual: m.halstead_difficulty,
            limit,
            breakdown: Vec::new(),
            message: format!(
                "Halstead difficulty is {:.1} (budget: {:.1})",
                m.halstead_difficulty, limit
            ),
            recommendation: format!(
                "Simplify operators/operands in `{}`: extract helpers, reduce distinct operators.",
                m.name
            ),
        });
    }

    if let Some(limit) = budgets.max_statements
        && m.statements > limit
    {
        violations.push(ComplexityViolation {
            file: m.file.clone(),
            function_name: m.name.clone(),
            line_number: m.start_line,
            end_line: m.end_line,
            metric: "Statement Count".to_string(),
            actual: m.statements as f64,
            limit: limit as f64,
            breakdown: Vec::new(),
            message: format!(
                "Function has {} statements (budget: {})",
                m.statements, limit
            ),
            recommendation: format!("Split `{}` into smaller focused functions.", m.name),
        });
    }

    if let Some(limit) = budgets.max_abc
        && m.abc_score > limit
    {
        violations.push(ComplexityViolation {
            file: m.file.clone(),
            function_name: m.name.clone(),
            line_number: m.start_line,
            end_line: m.end_line,
            metric: "ABC Score".to_string(),
            actual: m.abc_score,
            limit,
            breakdown: Vec::new(),
            message: format!("ABC score is {:.1} (budget: {:.1})", m.abc_score, limit),
            recommendation: format!(
                "Reduce assignments/branches/calls in `{}` by extracting helpers.",
                m.name
            ),
        });
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
    let name = extract_function_name(node, ctx.source, ctx.lang)?;
    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let lines = end_line - start_line + 1;
    let parameters = count_parameters(node, ctx.lang);

    let walker_ctx = WalkerContext {
        source: ctx.source,
        lang: ctx.lang,
    };

    let mut state = AnalysisState::new();
    walk_node(node, &walker_ctx, 0, &mut state);

    let distinct_ops = state.operators.len() as f64;
    let distinct_opds = state.operands.len() as f64;
    let halstead_difficulty = if distinct_opds > 0.0 {
        (distinct_ops / 2.0) * (state.total_operands as f64 / distinct_opds)
    } else {
        0.0
    };

    Some(FunctionMetrics {
        name,
        file: ctx.file_path.to_path_buf(),
        start_line,
        end_line,
        lines,
        parameters,
        cyclomatic: state.cyclomatic,
        cognitive: state.cognitive,
        halstead_difficulty,
        max_nesting_depth: state.max_nesting_depth,
        statements: state.statements,
        abc_score: abc_score(state.assignments, state.branches, state.calls),
        cognitive_breakdown: state.cognitive_breakdown,
        cyclomatic_breakdown: state.cyclomatic_breakdown,
    })
}

fn extract_function_name(node: Node, source: &[u8], lang: SupportedLanguage) -> Option<String> {
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

fn count_parameters(node: Node, lang: SupportedLanguage) -> usize {
    let param_kind = match lang {
        SupportedLanguage::Rust | SupportedLanguage::Python => "parameters",
        SupportedLanguage::TypeScript | SupportedLanguage::Tsx | SupportedLanguage::JavaScript => {
            "formal_parameters"
        }
        SupportedLanguage::Go => "parameter_list",
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
        .filter(|param| !matches!(param.kind(), "(" | ")" | "," | "{" | "}" | "[" | "]"))
        .count()
}
