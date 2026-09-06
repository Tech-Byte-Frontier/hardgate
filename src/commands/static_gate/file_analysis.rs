use super::*;

pub(super) fn analyze_loaded_files(
    analyzed_inputs: &[(&ClassifiedFile, &str)],
    request: &StaticRequest<'_>,
    ownership: &RustOwnership,
    report: &mut GateReport,
) -> Vec<FunctionMetrics> {
    let config = request.config;
    let root = request.root;
    let views = analyzed_inputs
        .iter()
        .flat_map(|(file, text)| ownership.views(file, text))
        .collect::<Vec<_>>();
    let anti_gaming = AntiGamingScanner::new(&config.anti_gaming);
    let invariants = InvariantsChecker::new(&config.invariants.rules);
    let context = FileAnalysisContext {
        config,
        root,
        anti_gaming: &anti_gaming,
        invariants: &invariants,
    };
    let analyzed = analyze_inputs(&views, &context);
    for view in &views {
        observations::observe_file(&view.file, config, report);
    }
    merge_file_analysis(analyzed, config, root, report)
}

struct FileAnalysis {
    role: FileRole,
    path: PathBuf,
    budgets: Vec<BudgetViolation>,
    suppressions: Vec<SuppressionViolation>,
    invariants: Vec<InvariantViolation>,
    functions: Vec<FunctionMetrics>,
    complexity: Vec<ComplexityViolation>,
    parse_error: Option<String>,
    size: Option<crate::engines::complexity::SizeBreakdown>,
}

struct FileAnalysisContext<'a> {
    config: &'a HardgateConfig,
    root: &'a Path,
    anti_gaming: &'a AntiGamingScanner,
    invariants: &'a InvariantsChecker,
}

fn analyze_inputs(inputs: &[RoleView], context: &FileAnalysisContext<'_>) -> Vec<FileAnalysis> {
    if inputs.len() < 8 {
        inputs
            .iter()
            .map(|view| analyze_one(view, context))
            .collect()
    } else {
        inputs
            .par_iter()
            .map(|view| analyze_one(view, context))
            .collect()
    }
}

fn analyze_one(view: &RoleView, context: &FileAnalysisContext<'_>) -> FileAnalysis {
    let file = &view.file;
    let (mut budgets, suppressions, invariants) = analyze_safety(view, context);
    let ComplexityAnalysis {
        functions,
        violations: complexity,
        parse_error,
        mut size,
    } = analyze_complexity(view, context);
    if let Some(size) = &mut size {
        let padding_lines = size.physical_lines.saturating_sub(view.lines);
        size.physical_lines = view.lines;
        size.blank_lines = size.blank_lines.saturating_sub(padding_lines);
        for budget in &mut budgets {
            budget
                .message
                .push_str(&format!("; {}", size.description()));
        }
    }
    FileAnalysis {
        role: file.role,
        path: file.path.clone(),
        budgets,
        suppressions,
        invariants,
        functions,
        complexity,
        parse_error,
        size,
    }
}

fn analyze_safety(
    view: &RoleView,
    context: &FileAnalysisContext<'_>,
) -> (
    Vec<BudgetViolation>,
    Vec<SuppressionViolation>,
    Vec<InvariantViolation>,
) {
    let file = &view.file;
    let content = view.text.as_str();
    let path = &file.path;
    let safety = file.role.receives_safety_checks();
    let budgets = if safety {
        let policy = effective_file_budgets(context.config, file.role);
        crate::engines::budgets::check_measured_budgets(
            path,
            (view.bytes, view.lines),
            &policy,
            context.root,
        )
    } else {
        Vec::new()
    };
    let suppressions = if safety && context.config.anti_gaming.disallow_suppressions {
        context
            .anti_gaming
            .scan_content(path, content, context.root)
    } else {
        Vec::new()
    };
    let invariants = if receives_invariants(file) && context.config.invariants.enforce {
        context.invariants.check_file(path, content, context.root)
    } else {
        Vec::new()
    };
    (budgets, suppressions, invariants)
}

pub(super) fn receives_invariants(file: &ClassifiedFile) -> bool {
    matches!(file.role, FileRole::Source | FileRole::Test)
}

#[derive(Default)]
struct ComplexityAnalysis {
    functions: Vec<FunctionMetrics>,
    violations: Vec<ComplexityViolation>,
    parse_error: Option<String>,
    size: Option<crate::engines::complexity::SizeBreakdown>,
}

fn analyze_complexity(view: &RoleView, context: &FileAnalysisContext<'_>) -> ComplexityAnalysis {
    let file = &view.file;
    if !file.role.receives_complexity() || !file.ast_supported {
        return ComplexityAnalysis::default();
    }
    let path = &file.path;
    let mut analyzer = ComplexityAnalyzer::new();
    let parsed = analyzer.analyze_role_structure(
        path,
        (
            view.syntax_source.as_deref().unwrap_or(&view.text),
            &view.text,
        ),
        context.root,
    );
    let structure = match parsed {
        Ok(structure) => structure,
        Err(error) => {
            return ComplexityAnalysis {
                parse_error: Some(error.to_string()),
                ..Default::default()
            };
        }
    };
    let mut functions = structure.functions;
    for function in &mut functions {
        function.test_only = file.role == FileRole::Test;
    }
    let policy = effective_function_budgets(context.config, file.role);
    let violations = ComplexityAnalyzer::check_violations(&functions, &policy);
    ComplexityAnalysis {
        functions,
        violations,
        parse_error: None,
        size: structure.size,
    }
}

fn merge_file_analysis(
    analyzed: Vec<FileAnalysis>,
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) -> Vec<FunctionMetrics> {
    let mut all_functions = Vec::new();
    for file in analyzed {
        if let Some(size) = file.size {
            report.file_sizes.push(crate::diagnostics::FileSizeMetrics {
                file: file
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&file.path)
                    .to_path_buf(),
                role: file.role,
                size,
            });
        }
        apply_budget_findings(report, config, file.role, file.budgets);
        apply_suppression_findings(report, config, file.role, file.suppressions);
        apply_invariant_findings(report, config, file.role, file.invariants);
        apply_complexity_findings(report, config, file.role, file.complexity);
        all_functions.extend(file.functions);
        if let Some(error) = file.parse_error {
            record_role_evidence_failure(
                report,
                RoleEvidence {
                    config,
                    role: file.role,
                    step: "parse-source",
                    target: &file.path,
                    message: error,
                },
            );
        }
    }
    all_functions
}

/// Shared single-file analysis used by `scan` and the MCP server.
pub struct AnalyzeInput<'a> {
    pub path: &'a Path,
    pub content: &'a str,
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub anti_gaming: &'a AntiGamingScanner,
    pub invariants: &'a InvariantsChecker,
}

pub fn analyze_file_content(input: AnalyzeInput, report: &mut GateReport) -> Vec<FunctionMetrics> {
    let classified = match classify_file(input.path, input.config, input.root) {
        Ok(file) => file,
        Err(error) => {
            record_evidence_failure(
                report,
                true,
                EvidenceFailure {
                    step: "classify-source",
                    target: input.path,
                    message: format!("Unable to classify file: {error}"),
                },
            );
            return Vec::new();
        }
    };
    record_classification_gaps(&[&classified], input.config, input.root, report);
    let context = FileAnalysisContext {
        config: input.config,
        root: input.root,
        anti_gaming: input.anti_gaming,
        invariants: input.invariants,
    };
    let ownership = if input.path.is_file() && RustOwnership::context_path(input.path) {
        match RustOwnership::from_root(input.root, input.config, &[(&classified, input.content)]) {
            Ok(ownership) => ownership,
            Err(error) => {
                record_evidence_failure(
                    report,
                    true,
                    EvidenceFailure {
                        step: "rust-test-ownership",
                        target: input.path,
                        message: format!("Unable to establish Rust module ownership: {error:#}"),
                    },
                );
                RustOwnership::from_inputs(&[(&classified, input.content)])
            }
        }
    } else {
        RustOwnership::from_inputs(&[(&classified, input.content)])
    };
    let views = ownership.views(&classified, input.content);
    let analyzed = analyze_inputs(&views, &context);
    for view in &views {
        observations::observe_file(&view.file, input.config, report);
    }
    merge_file_analysis(analyzed, input.config, input.root, report)
}
