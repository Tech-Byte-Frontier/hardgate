use super::{GraphInput, graph_eligible, run_graph};
use crate::commands::evidence::{EvidenceFailure, record_evidence_failure};
use crate::commands::role_policy::classify_files;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::{DiscoverOptions, discover_files};
use anyhow::Result;
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub(crate) struct DeadCodeScope<'a> {
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub selected: &'a [PathBuf],
    pub read_results: &'a [(PathBuf, String)],
}

/// Scope selects findings; every discoverable reference remains graph context.
/// Reuse selected bytes already analyzed by the static gate.
pub(crate) fn run_scoped_dead_code_analysis(
    scope: DeadCodeScope<'_>,
    report: &mut GateReport,
) -> Result<()> {
    let mut paths = discover_files(DiscoverOptions {
        root: scope.root,
        diff_only: false,
        exclusions: &scope.config.budgets.files.exclusions.paths,
    })?;
    paths.extend_from_slice(scope.selected);
    paths.sort();
    paths.dedup();
    let classified = classify_files(&paths, scope.config, scope.root)?;
    let cached: HashMap<&Path, &str> = scope
        .read_results
        .iter()
        .map(|(path, text)| (path.as_path(), text.as_str()))
        .collect();
    let mut inputs = Vec::new();
    for file in classified.into_iter().filter(graph_eligible) {
        let text = if let Some(text) = cached.get(file.path.as_path()) {
            Cow::Borrowed(*text)
        } else if scope.selected.contains(&file.path) {
            // The static gate already recorded this selected file's read failure.
            continue;
        } else {
            let Some(text) = read_reference(&file.path, report) else {
                continue;
            };
            Cow::Owned(text)
        };
        inputs.push((file, text));
    }
    let borrowed = inputs
        .iter()
        .map(|(file, text)| (file, text.as_ref()))
        .collect::<Vec<_>>();
    let selected = scope
        .selected
        .iter()
        .map(|path| path.strip_prefix(scope.root).unwrap_or(path).to_path_buf())
        .collect::<BTreeSet<_>>();
    run_graph(
        GraphInput {
            config: scope.config,
            root: scope.root,
            sources: &borrowed,
            selected: Some(&selected),
        },
        report,
    );
    Ok(())
}

fn read_reference(path: &Path, report: &mut GateReport) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) => {
            record_evidence_failure(
                report,
                true,
                EvidenceFailure {
                    step: "dead-code-context",
                    target: path,
                    message: format!("Cannot read a required repository reference: {error}"),
                },
            );
            None
        }
    }
}
