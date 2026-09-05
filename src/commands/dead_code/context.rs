use super::{GraphInput, graph_eligible, run_graph};
use crate::commands::evidence::{EvidenceFailure, record_evidence_failure};
use crate::commands::source_snapshot::SourceSnapshot;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(crate) struct DeadCodeScope<'a> {
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub selected: &'a [PathBuf],
    pub snapshot: &'a SourceSnapshot,
}

/// All references and selected sources come from the same immutable capture.
pub(crate) fn run_scoped_dead_code_analysis(
    scope: DeadCodeScope<'_>,
    report: &mut GateReport,
) -> Result<()> {
    let mut inputs = Vec::new();
    for source in &scope.snapshot.files {
        let file = &source.classified;
        if !graph_eligible(file) {
            continue;
        }
        match &source.content {
            Ok(text) => inputs.push((file, text.as_ref())),
            Err(error) if !scope.selected.contains(&file.path) => record_evidence_failure(
                report,
                true,
                EvidenceFailure {
                    step: "dead-code-context",
                    target: &file.path,
                    message: format!("Cannot read a required repository reference: {error}"),
                },
            ),
            Err(_) => {} // The static gate already recorded selected read failures.
        }
    }
    let selected = scope
        .selected
        .iter()
        .map(|path| path.strip_prefix(scope.root).unwrap_or(path).to_path_buf())
        .collect::<BTreeSet<_>>();
    run_graph(
        GraphInput {
            config: scope.config,
            root: scope.root,
            sources: &inputs,
            selected: Some(&selected),
        },
        report,
    );
    Ok(())
}
