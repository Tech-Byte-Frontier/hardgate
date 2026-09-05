mod context;
pub(crate) use context::{DeadCodeScope, run_scoped_dead_code_analysis};

use super::role_policy::{apply_dead_code_findings, classify_files};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{ClassifiedFile, FileRole};
use crate::engines::DeadCodeAnalyzer;
use anyhow::Result;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Run the configured dead-code analyzer over successfully read files.
///
/// The analyzer consumes a complete file graph, while role policy decides
/// whether each finding is blocking, advisory, or ignored.  Keeping that
/// policy application here lets `check`, `verify`, and legacy baselines share
/// exactly the same dead-code semantics.
pub(crate) fn run_dead_code_analysis(
    config: &HardgateConfig,
    read_results: &[(PathBuf, String)],
    root: &Path,
    report: &mut GateReport,
) -> Result<()> {
    let paths = read_results
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let classified = classify_files(&paths, config, root)?;
    let inputs = classified
        .iter()
        .zip(read_results)
        .filter(|(file, _)| graph_eligible(file))
        .map(|(file, (_, text))| (file, text.as_str()))
        .collect::<Vec<_>>();
    run_graph(
        GraphInput {
            config,
            root,
            sources: &inputs,
            selected: None,
        },
        report,
    );
    Ok(())
}

struct GraphInput<'a> {
    config: &'a HardgateConfig,
    root: &'a Path,
    sources: &'a [(&'a ClassifiedFile, &'a str)],
    selected: Option<&'a BTreeSet<PathBuf>>,
}

fn graph_eligible(file: &ClassifiedFile) -> bool {
    file.ast_supported
        && matches!(
            file.role,
            FileRole::Source | FileRole::Test | FileRole::Generated | FileRole::Fixture
        )
}

fn run_graph(input: GraphInput<'_>, report: &mut GateReport) {
    let files = input
        .sources
        .iter()
        .map(|(file, _)| file.path.clone())
        .collect::<Vec<_>>();
    let contents = input
        .sources
        .iter()
        .map(|(file, text)| (file.path.clone(), *text))
        .collect::<Vec<_>>();
    let roles: HashMap<&Path, FileRole> = input
        .sources
        .iter()
        .map(|(file, _)| (relative_path(&file.path, input.root), file.role))
        .collect();
    let analyzer = DeadCodeAnalyzer::new(&input.config.analysis.dead_code);
    for finding in analyzer.analyze_borrowed(&files, &contents, input.root) {
        if input
            .selected
            .is_some_and(|paths| !paths.contains(&finding.file))
        {
            continue;
        }
        let role = roles
            .get(finding.file.as_path())
            .copied()
            .unwrap_or(FileRole::Unknown);
        apply_dead_code_findings(report, input.config, role, vec![finding]);
    }
}

fn relative_path<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::run_dead_code_analysis;
    use crate::config::HardgateConfig;
    use crate::diagnostics::GateReport;
    use std::path::Path;
    use std::path::PathBuf;

    #[test]
    fn dead_code_graph_ignores_config_but_reports_unreferenced_source() {
        let config = HardgateConfig::default();
        let contents = vec![
            (PathBuf::from("src/unused.rs"), "fn unused() {}".to_string()),
            (
                PathBuf::from("src/generated/unused.ts"),
                "export function generatedOnly() {}".to_string(),
            ),
            (PathBuf::from("package.json"), "{}".to_string()),
            (PathBuf::from("Cargo.toml"), "[package]".to_string()),
        ];
        let mut report = GateReport::new("test".to_string());
        run_dead_code_analysis(&config, &contents, Path::new("."), &mut report).unwrap();
        assert!(
            report
                .dead_code_violations
                .iter()
                .any(|finding| finding.file.as_path() == Path::new("src/unused.rs"))
        );
        assert!(
            report
                .dead_code_violations
                .iter()
                .all(|finding| finding.file.as_path() != Path::new("package.json"))
        );
        assert!(
            report
                .dead_code_violations
                .iter()
                .all(|finding| finding.file.as_path() != Path::new("Cargo.toml"))
        );
        assert!(
            report
                .dead_code_violations
                .iter()
                .all(|finding| finding.file.as_path() != Path::new("src/generated/unused.ts"))
        );
    }
}
