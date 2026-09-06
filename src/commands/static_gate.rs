mod file_analysis;
pub use file_analysis::{AnalyzeInput, analyze_file_content};
use file_analysis::{analyze_loaded_files, receives_invariants};
mod classification_gaps;
use classification_gaps::record_classification_gaps;
mod excerpts;
mod observations;
mod selection;
#[cfg(test)]
mod snapshot_tests;

use super::evidence::{EvidenceFailure, record_evidence_failure};
use super::role_policy::{
    CloneRun, RoleEvidence, apply_budget_findings, apply_complexity_findings,
    apply_invariant_findings, apply_suppression_findings, classify_file, classify_files,
    effective_file_budgets, effective_function_budgets, record_role_evidence_failure,
    run_clone_analysis,
};
use super::source_snapshot::{SharedSource, SourceSnapshot};
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::rust_ownership::{RoleView, RustOwnership};
use crate::discovery::{ClassifiedFile, DiscoverOptions, FileRole, discover_paths};
use crate::engines::{
    AntiGamingScanner, BudgetViolation, ComplexityAnalyzer, ComplexityViolation, FunctionMetrics,
    InvariantViolation, InvariantsChecker, SuppressionViolation,
};
use anyhow::Result;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Artifacts of one static-gate run: the report plus the discovered files,
/// their contents, and per-function metrics for downstream gates.
pub type StaticGateOutcome = Option<(
    GateReport,
    Vec<PathBuf>,
    Vec<(PathBuf, String)>,
    Vec<FunctionMetrics>,
)>;

/// Static-gate artifacts computed directly from a Git snapshot's contents.
pub type StaticSnapshotOutcome = (
    GateReport,
    Vec<PathBuf>,
    Vec<(PathBuf, String)>,
    Vec<FunctionMetrics>,
);

/// Run the static gate over the whole discovered tree.
pub fn run_static_gate(config: &HardgateConfig, diff: bool) -> Result<StaticGateOutcome> {
    run_static_gate_scoped(config, diff, &[])
}

pub fn run_static_gate_snapshot(
    config: &HardgateConfig,
    contents: &[(PathBuf, String)],
) -> Result<StaticSnapshotOutcome> {
    let root = Path::new(".");
    let files = contents
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let classified = classify_files(&files, config, root)?;
    let snapshot = SourceSnapshot::from_shared(
        classified
            .into_iter()
            .zip(contents)
            .map(|(file, (_, text))| (file, Arc::from(text.as_str())))
            .collect(),
    );
    let request = StaticRequest {
        config,
        root,
        paths: &[],
        diff: false,
        snippets: false,
    };
    let outcome = analyze_snapshot(request, files, Vec::new(), snapshot)?;
    Ok((
        outcome.report,
        outcome.files,
        contents.to_vec(),
        outcome.functions,
    ))
}

pub(crate) struct StaticRequest<'a> {
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
    pub paths: &'a [PathBuf],
    pub diff: bool,
    pub snippets: bool,
}

pub(crate) struct StaticAnalysis {
    pub report: GateReport,
    pub files: Vec<PathBuf>,
    pub read_results: Vec<SharedSource>,
    pub functions: Vec<FunctionMetrics>,
    pub ownership: RustOwnership,
    pub empty: bool,
}

/// Run the static gate, optionally scoped to explicit files or directories.
pub fn run_static_gate_scoped(
    config: &HardgateConfig,
    diff: bool,
    paths: &[PathBuf],
) -> Result<StaticGateOutcome> {
    run_static_gate_at(config, diff, paths, Path::new("."))
}

pub fn run_static_gate_at(
    config: &HardgateConfig,
    diff: bool,
    paths: &[PathBuf],
    root: &Path,
) -> Result<StaticGateOutcome> {
    let run = run_shared_gate(StaticRequest {
        config,
        root,
        paths,
        diff,
        snippets: false,
    })?;
    if run.empty {
        return Ok(None);
    }
    let contents = run
        .read_results
        .into_iter()
        .map(|(path, text)| (path, text.to_string()))
        .collect();
    Ok(Some((run.report, run.files, contents, run.functions)))
}

pub(crate) fn run_shared_gate(request: StaticRequest<'_>) -> Result<StaticAnalysis> {
    let reference_context = request.diff && clone_context_enabled(request.config);
    let full = if reference_context || request.diff || !request.paths.is_empty() {
        Some(discover_paths(DiscoverOptions {
            root: request.root,
            diff_only: false,
            exclusions: &request.config.budgets.files.exclusions.paths,
        })?)
    } else {
        None
    };
    let discovery = if !request.diff
        && let Some(full) = &full
    {
        crate::discovery::DiscoveryResult {
            files: full.files.clone(),
            excluded_files: full.excluded_files.clone(),
            classified_files: Vec::new(),
        }
    } else {
        discover_paths(DiscoverOptions {
            root: request.root,
            diff_only: request.diff,
            exclusions: &request.config.budgets.files.exclusions.paths,
        })?
    };
    let (files, excluded) = selection::select_files(
        selection::Scope {
            config: request.config,
            diff: request.diff,
            paths: request.paths,
            root: request.root,
            full: full.as_ref(),
        },
        discovery,
    )?;
    if files.is_empty() {
        return analyze_snapshot(request, files, excluded, SourceSnapshot::default());
    }
    let mut context_paths = if reference_context {
        full.as_ref()
            .map(|full| full.files.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if let Some(full) = &full {
        context_paths.extend(
            full.files
                .iter()
                .chain(&full.excluded_files)
                .filter(|path| RustOwnership::context_path(path))
                .cloned(),
        );
    }
    context_paths.extend(
        excluded
            .iter()
            .filter(|path| RustOwnership::context_path(path))
            .cloned(),
    );
    context_paths.extend_from_slice(&files);
    context_paths.sort();
    context_paths.dedup();
    let classified = classify_files(&context_paths, request.config, request.root)?;
    analyze_snapshot(
        request,
        files,
        excluded,
        SourceSnapshot::capture(classified),
    )
}

fn clone_context_enabled(config: &HardgateConfig) -> bool {
    FileRole::POLICY_ROLES.into_iter().any(|role| {
        let override_enabled = config
            .roles
            .for_role(role)
            .and_then(|policy| policy.clone_enabled);
        (role.receives_clone_analysis() || override_enabled == Some(true))
            && override_enabled.unwrap_or(config.clones.enabled)
    })
}

fn analyze_snapshot(
    request: StaticRequest<'_>,
    files: Vec<PathBuf>,
    excluded: Vec<PathBuf>,
    snapshot: SourceSnapshot,
) -> Result<StaticAnalysis> {
    let ownership_inputs = snapshot
        .files
        .iter()
        .filter_map(|file| {
            file.content
                .as_ref()
                .ok()
                .map(|text| (&file.classified, text.as_ref()))
        })
        .collect::<Vec<_>>();
    let mut ownership = RustOwnership::from_inputs(&ownership_inputs);
    if snapshot
        .files
        .iter()
        .any(|file| RustOwnership::context_path(&file.classified.path) && file.content.is_err())
    {
        ownership.disable_module_proof();
    }
    let mut report = GateReport::new(request.config.gate.name.clone());
    record_budget_exclusion_advisory(&excluded, &mut report);
    let selected = files
        .iter()
        .filter_map(|path| snapshot.find(path))
        .collect::<Vec<_>>();
    let classified = selected
        .iter()
        .map(|file| &file.classified)
        .collect::<Vec<_>>();
    record_classification_gaps(&classified, request.config, request.root, &mut report);
    let mut loaded = Vec::new();
    for file in selected {
        match &file.content {
            Ok(text) => loaded.push((&file.classified, text.as_ref())),
            Err(error) => record_role_evidence_failure(
                &mut report,
                RoleEvidence {
                    config: request.config,
                    role: file.classified.role,
                    step: "read-source",
                    target: &file.classified.path,
                    message: format!("Unable to read classified file: {error}"),
                },
            ),
        }
    }
    let functions = analyze_loaded_files(&loaded, &request, &ownership, &mut report);
    run_clone_analysis(
        CloneRun {
            snapshot: &snapshot,
            ownership: &ownership,
            selected_ids: &snapshot.selected_ids(&files),
            changed_files: &files,
            config: request.config,
            root: request.root,
            diff: request.diff,
        },
        &mut report,
    )?;

    if request.snippets {
        excerpts::capture(&snapshot, request.root, &mut report);
    }
    Ok(StaticAnalysis {
        report,
        read_results: snapshot.shared_contents(&files),
        functions,
        ownership,
        empty: files.is_empty(),
        files,
    })
}

fn record_budget_exclusion_advisory(excluded_files: &[PathBuf], report: &mut GateReport) {
    if excluded_files.is_empty() {
        return;
    }
    let count = excluded_files.len();
    let noun = if count == 1 { "file" } else { "files" };
    report.advisories.push(format!(
        "{} {} excluded from file budget checks via hardgate.toml.",
        count, noun
    ));
}
