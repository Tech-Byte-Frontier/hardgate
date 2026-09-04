use super::dead_code::run_dead_code_analysis;
use super::evidence::{EvidenceFailure, record_evidence_failure};
use super::role_policy::classify_file;
use super::static_gate::{run_static_gate_scoped, run_static_gate_snapshot};
use crate::adoption::apply_legacy_ratchet;
use crate::config::HardgateConfig;
use crate::diagnostics::GateReport;
use crate::discovery::FileRole;
use crate::engines::{FunctionMetrics, run_generated_freshness as execute_generated_freshness};
use crate::git_evidence::{ChangedLineMap, ReferenceEvidence, load_reference};
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Static artifacts with an explicit empty marker so command callers can keep
/// emitting reports when discovery finds no files.
pub(crate) struct GateRun {
    pub report: GateReport,
    pub files: Vec<PathBuf>,
    pub read_results: Vec<(PathBuf, String)>,
    pub functions: Vec<FunctionMetrics>,
    pub empty: bool,
}

pub(crate) fn run_static_gate_or_empty(
    config: &HardgateConfig,
    diff: bool,
    paths: &[PathBuf],
) -> Result<GateRun> {
    let outcome = run_static_gate_scoped(config, diff, paths)?;
    let Some((report, files, read_results, functions)) = outcome else {
        return Ok(GateRun {
            report: GateReport::new(config.gate.name.clone()),
            files: Vec::new(),
            read_results: Vec::new(),
            functions: Vec::new(),
            empty: true,
        });
    };
    Ok(GateRun {
        report,
        files,
        read_results,
        functions,
        empty: false,
    })
}

/// Human-readable discovery context retained as a report advisory so JSON
/// output remains a single parseable document even for empty runs.
pub(crate) fn empty_discovery_advisory(diff: bool, scoped: bool) -> String {
    if scoped {
        "no matching source files detected for the given path(s).".to_string()
    } else if diff {
        "no git-modified source files detected to check.".to_string()
    } else {
        "no matching source files detected.".to_string()
    }
}

/// Record the configured generated-artifact freshness result in the report.
///
/// Freshness is a required gate whenever enabled, including when source
/// discovery produced no files.  Successful runs become concise evidence;
/// failed runs remain blocking orchestration findings and are never subject to
/// the legacy static-debt ratchet.
pub(crate) fn run_generated_freshness(
    config: &HardgateConfig,
    root: &Path,
    report: &mut GateReport,
) {
    let Some(result) = execute_generated_freshness(&config.generated, root) else {
        return;
    };
    match result {
        Ok(result) => report.advisories.push(format!(
            "generated-freshness evidence: `{}` completed successfully.",
            result.command
        )),
        Err(violation) => report.orchestration_violations.push(violation),
    }
}

/// Apply legacy static-debt adoption against the configured reference.
///
/// The returned evidence can be reused by `check --diff` so the changed-line
/// map and baseline always come from one Git snapshot.  Coverage, mutation,
/// generated freshness, and orchestration findings are intentionally run by
/// callers after this function and therefore cannot be ratcheted.
pub(crate) fn run_legacy_ratchet(
    config: &HardgateConfig,
    root: &Path,
    current: &mut GateReport,
    include_dead_code: bool,
) -> Option<ReferenceEvidence> {
    if !config.legacy.ratchet {
        return None;
    }

    let Some(reference) = config.legacy.reference_branch.as_deref() else {
        let summary = LegacySummary::new("<missing-reference>", current.total_violations());
        record_legacy_failure(
            current,
            &summary.reference,
            "legacy.ratchet is enabled but legacy.reference_branch is missing".to_string(),
        );
        push_legacy_summary(current, &summary);
        return None;
    };

    match load_reference(root, reference) {
        Ok(loaded) => {
            let summary = apply_legacy_baseline(LegacyBaselineRequest {
                config,
                root,
                current,
                evidence: &loaded,
                include_dead_code,
            });
            push_legacy_summary(current, &summary);
            Some(loaded)
        }
        Err(error) => {
            let summary = LegacySummary::new(reference, current.total_violations());
            record_legacy_failure(
                current,
                reference,
                format!("Unable to load legacy Git reference evidence: {error}"),
            );
            push_legacy_summary(current, &summary);
            None
        }
    }
}

struct LegacySummary {
    reference: String,
    merge_base: String,
    grandfathered: usize,
    retained: usize,
}

impl LegacySummary {
    fn new(reference: &str, retained: usize) -> Self {
        Self {
            reference: reference.to_string(),
            merge_base: "<unavailable>".to_string(),
            grandfathered: 0,
            retained,
        }
    }
}

fn apply_legacy_baseline(request: LegacyBaselineRequest<'_>) -> LegacySummary {
    let LegacyBaselineRequest {
        config,
        root,
        current,
        evidence,
        include_dead_code,
    } = request;
    let mut summary = LegacySummary {
        reference: config
            .legacy
            .reference_branch
            .clone()
            .unwrap_or_else(|| "<missing-reference>".to_string()),
        merge_base: evidence.change_set.merge_base.clone(),
        grandfathered: 0,
        retained: current.total_violations(),
    };
    let baseline_contents: Vec<(PathBuf, String)> = evidence
        .snapshot
        .files
        .iter()
        .map(|(path, content)| (path.clone(), content.clone()))
        .collect();
    let (mut baseline, _files, baseline_read, _functions) =
        match run_static_gate_snapshot(config, &baseline_contents) {
            Ok(result) => result,
            Err(error) => {
                record_legacy_failure(
                    current,
                    &summary.reference,
                    format!("Unable to analyze the legacy baseline static snapshot: {error}"),
                );
                return summary;
            }
        };
    if include_dead_code {
        if let Err(error) = run_dead_code_analysis(config, &baseline_read, root, &mut baseline) {
            record_legacy_failure(
                current,
                &summary.reference,
                format!("Unable to analyze the legacy baseline: {error}"),
            );
        }
    }
    let outcome = apply_legacy_ratchet(current, &baseline, &evidence.change_set);
    summary.grandfathered = outcome.grandfathered;
    summary.retained = outcome.retained;
    summary
}

struct LegacyBaselineRequest<'a> {
    config: &'a HardgateConfig,
    root: &'a Path,
    current: &'a mut GateReport,
    evidence: &'a ReferenceEvidence,
    include_dead_code: bool,
}

fn record_legacy_failure(report: &mut GateReport, reference: &str, message: String) {
    record_evidence_failure(
        report,
        true,
        EvidenceFailure {
            step: "legacy-ratchet",
            target: Path::new(reference),
            message,
        },
    );
}

fn push_legacy_summary(report: &mut GateReport, summary: &LegacySummary) {
    report.advisories.push(format!(
        "legacy ratchet: reference=`{}` merge-base=`{}` grandfathered={} retained={}",
        summary.reference, summary.merge_base, summary.grandfathered, summary.retained,
    ));
}

/// Inputs for filtering changed-line coverage to the current selected source.
pub(crate) struct ChangedLineFilter<'a> {
    pub changed_lines: &'a ChangedLineMap,
    pub selected_files: &'a [PathBuf],
    pub read_results: &'a [(PathBuf, String)],
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
}

/// Keep only changed lines belonging to successfully read, selected,
/// AST-supported Source-role files.
pub(crate) fn filter_changed_lines(request: ChangedLineFilter<'_>) -> Result<ChangedLineMap> {
    let selected: BTreeSet<String> = request
        .selected_files
        .iter()
        .map(|path| normalized_key(path, request.root))
        .collect();
    let mut source_files = BTreeSet::new();
    for (path, _) in request.read_results {
        let key = normalized_key(path, request.root);
        if !selected.contains(&key) {
            continue;
        }
        let classified = classify_file(path, request.config)?;
        if classified.ast_supported && classified.role == FileRole::Source {
            source_files.insert(key);
        }
    }

    Ok(request
        .changed_lines
        .iter()
        .filter(|(path, _)| source_files.contains(&normalized_key(path, request.root)))
        .map(|(path, lines)| (path.clone(), lines.clone()))
        .collect())
}

fn normalized_key(path: &Path, root: &Path) -> String {
    let root_absolute = root.canonicalize().ok();
    let candidate = if path.is_absolute() {
        root_absolute
            .as_deref()
            .and_then(|absolute| path.strip_prefix(absolute).ok())
            .unwrap_or(path)
    } else {
        path
    };
    let mut parts = Vec::new();
    for component in candidate.components() {
        if let std::path::Component::Normal(part) = component {
            parts.push(part.to_string_lossy().into_owned());
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::{ChangedLineFilter, filter_changed_lines};
    use crate::config::HardgateConfig;
    use crate::git_evidence::ChangedLineMap;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    #[test]
    fn changed_lines_are_limited_to_read_source_files() {
        let changed = ChangedLineMap::from([
            (PathBuf::from("src/lib.rs"), BTreeSet::from([1])),
            (PathBuf::from("tests/lib.rs"), BTreeSet::from([1])),
            (PathBuf::from("src/missing.rs"), BTreeSet::from([1])),
        ]);
        let selected = vec![PathBuf::from("./src/lib.rs"), PathBuf::from("tests/lib.rs")];
        let read = vec![
            (PathBuf::from("./src/lib.rs"), "fn source() {}".to_string()),
            (PathBuf::from("tests/lib.rs"), "fn test() {}".to_string()),
        ];
        let filtered = filter_changed_lines(ChangedLineFilter {
            changed_lines: &changed,
            selected_files: &selected,
            read_results: &read,
            config: &HardgateConfig::default(),
            root: Path::new("."),
        })
        .unwrap();
        assert_eq!(
            filtered,
            BTreeMap::from([(PathBuf::from("src/lib.rs"), BTreeSet::from([1]))])
        );
    }
}
