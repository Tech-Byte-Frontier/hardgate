use super::role_policy::{apply_dead_code_findings, classify_file};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::FileRole;
use crate::engines::DeadCodeAnalyzer;
use anyhow::Result;
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
    let mut graph_files = Vec::new();
    let mut graph_contents = Vec::new();
    let mut graph_roles = Vec::new();
    for (path, content) in read_results {
        let classified = classify_file(path, config)?;
        if !classified.ast_supported
            || !matches!(
                classified.role,
                FileRole::Source | FileRole::Test | FileRole::Generated | FileRole::Fixture
            )
        {
            continue;
        }
        graph_files.push(path.clone());
        graph_contents.push((path.clone(), content.clone()));
        graph_roles.push(classified);
    }
    let analyzer = DeadCodeAnalyzer::new(&config.analysis.dead_code);
    let findings = analyzer.analyze(&graph_files, &graph_contents, root);
    for finding in findings {
        let role = graph_roles
            .iter()
            .find(|file| relative_path(&file.path, root) == finding.file)
            .map(|file| file.role)
            .unwrap_or(FileRole::Unknown);
        apply_dead_code_findings(report, config, role, vec![finding]);
    }
    Ok(())
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
