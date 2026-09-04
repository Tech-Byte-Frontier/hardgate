use super::{SourceCoverageRequest, source_files_for_coverage};
use crate::config::{CoverageConfig, HardgateConfig};
use crate::diagnostics::GateReport;
use crate::engines::FunctionMetrics;
use crate::engines::coverage::{CoverageEvaluationScope, CoverageScorer, FileCoverage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn coverage_config() -> HardgateConfig {
    HardgateConfig {
        coverage: CoverageConfig {
            enabled: true,
            report: None,
            min_line_percent: Some(90.0),
            min_function_percent: None,
            min_branch_percent: None,
            max_crap_score: None,
            critical_paths: None,
        },
        ..HardgateConfig::default()
    }
}

fn function(file: &str) -> FunctionMetrics {
    FunctionMetrics {
        name: "main".to_string(),
        file: PathBuf::from(file),
        start_line: 1,
        end_line: 1,
        lines: 1,
        parameters: 0,
        cyclomatic: 1,
        cognitive: 0,
        halstead_difficulty: 0.0,
        max_nesting_depth: 0,
        statements: 1,
        abc_score: 0.0,
        cognitive_breakdown: Vec::new(),
        cyclomatic_breakdown: Vec::new(),
    }
}

fn covered(path: &str) -> FileCoverage {
    FileCoverage {
        file_path: PathBuf::from(path),
        lines_found: 1,
        lines_hit: 1,
        line_hits: HashMap::from([(1, 1)]),
        ..FileCoverage::default()
    }
}

fn source_files(
    files: &[PathBuf],
    functions: &[FunctionMetrics],
    config: &HardgateConfig,
) -> Vec<PathBuf> {
    let mut report = GateReport::new("verify".to_string());
    source_files_for_coverage(SourceCoverageRequest {
        files,
        functions,
        root: Path::new("."),
        config,
        report: &mut report,
    })
}

fn evaluate_scope(
    config: &HardgateConfig,
    source_files: &[PathBuf],
    functions: &[FunctionMetrics],
    coverage_map: &HashMap<PathBuf, FileCoverage>,
) -> Vec<crate::engines::coverage::CoverageViolation> {
    CoverageScorer::new(&config.coverage).evaluate_for_sources(
        coverage_map,
        functions,
        CoverageEvaluationScope {
            root: Path::new("."),
            source_files: Some(source_files),
        },
    )
}

#[test]
fn source_scope_skips_declaration_only_rust_and_requires_executable_rust() {
    let config = coverage_config();
    let files = vec![
        PathBuf::from("src/lib.rs"),
        PathBuf::from("./src/main.rs"),
        PathBuf::from("src/value.js"),
    ];
    let functions = vec![function("src/main.rs")];
    let source_files = source_files(&files, &functions, &config);

    assert_eq!(
        source_files,
        vec![
            PathBuf::from("./src/main.rs"),
            PathBuf::from("src/value.js")
        ]
    );

    let coverage_map = HashMap::from([(PathBuf::from("src/value.js"), covered("src/value.js"))]);
    let violations = evaluate_scope(&config, &source_files, &functions, &coverage_map);
    assert!(violations.iter().any(|violation| {
        violation.metric == "Missing Source Coverage"
            && violation.file == Path::new("./src/main.rs")
    }));
    assert!(!violations.iter().any(|violation| {
        violation.metric == "Missing Source Coverage" && violation.file == Path::new("src/lib.rs")
    }));
}

#[test]
fn source_scope_keeps_missing_javascript_source_required() {
    let config = coverage_config();
    let files = vec![PathBuf::from("src/value.js")];
    let source_files = source_files(&files, &[], &config);
    let violations = evaluate_scope(&config, &source_files, &[], &HashMap::new());
    assert!(violations.iter().any(|violation| {
        violation.metric == "Missing Source Coverage" && violation.file == Path::new("src/value.js")
    }));
}

#[test]
fn source_scope_uses_normalized_function_paths() {
    let config = coverage_config();
    let files = vec![PathBuf::from("src/main.rs")];
    let root = std::env::current_dir().expect("test root");
    let functions = vec![function(&root.join("src/main.rs").display().to_string())];
    let source_files = source_files(&files, &functions, &config);
    assert_eq!(source_files, files);
}
