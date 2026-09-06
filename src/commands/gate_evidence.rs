use super::check::CheckOptions;
use super::evidence::{EvidenceFailure, record_evidence_failure};
use super::role_policy::classify_files;
use super::source_snapshot::SharedSource;
pub(crate) use super::static_gate::StaticAnalysis as GateRun;
use super::static_gate::{StaticRequest, run_shared_gate, run_static_gate_snapshot};
use crate::adoption::apply_legacy_ratchet;
use crate::config::{HardgateConfig, Severity};
use crate::diagnostics::GateReport;
use crate::discovery::FileRole;
use crate::engines::{
    coverage::{normalized_repository_key, retain_code_lines},
    run_generated_freshness as execute_generated_freshness,
};
use crate::git_evidence::{ChangedLineMap, ReferenceEvidence, load_reference};
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(crate) fn run_static_gate_or_empty(request: StaticRequest<'_>) -> Result<GateRun> {
    run_shared_gate(request)
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

pub(crate) fn check_scope_advisory(_config: &HardgateConfig, opts: &CheckOptions) -> String {
    let omitted = [
        super::CheckKind::Policy,
        super::CheckKind::Format,
        super::CheckKind::Lint,
        super::CheckKind::Tests,
        super::CheckKind::Typecheck,
    ]
    .into_iter()
    .filter(|kind| !opts.selects(*kind))
    .map(|kind| format!("{kind:?}").to_ascii_lowercase())
    .collect::<Vec<_>>();
    if omitted.is_empty() {
        "All configured acceptance requirements were requested in the selected path scope; consult engine states for completion. Mutation reports describe the recorded specialist sample.".into()
    } else {
        format!(
            "Partial check: omitted {}. This result does not establish complete project acceptance; run `hardgate check` for all requirements.",
            omitted.join(", ")
        )
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
    report.observe_engine(
        crate::diagnostics::execution::EngineId::GeneratedFreshness,
        crate::diagnostics::execution::EngineState::Completed,
    );
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
            current.observe_engine(
                crate::diagnostics::execution::EngineId::LegacyRatchet,
                crate::diagnostics::execution::EngineState::Completed,
            );
            let summary = apply_legacy_baseline(LegacyBaselineRequest {
                config,
                current,
                evidence: &loaded,
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
        current,
        evidence,
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
    let baseline_config = trusted_baseline_config(config);
    let (baseline, _files, _baseline_read, _functions) =
        match run_static_gate_snapshot(&baseline_config, &baseline_contents) {
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

    if !baseline.orchestration_violations.is_empty() {
        record_legacy_failure(
            current,
            &summary.reference,
            format!(
                "Legacy baseline static analysis produced {} required evidence violation(s); cannot apply the ratchet.",
                baseline.orchestration_violations.len()
            ),
        );
        return summary;
    }
    let outcome = apply_legacy_ratchet(current, &baseline, &evidence.change_set);
    summary.grandfathered = outcome.grandfathered;
    summary.retained = outcome.retained;
    summary
}

/// A legacy snapshot is evidence, not a user-facing advisory run.  Force
/// evidence-producing roles to error severity so malformed or unsupported
/// baseline inputs can never be hidden by a non-strict current configuration.
fn trusted_baseline_config(config: &HardgateConfig) -> HardgateConfig {
    let mut baseline = config.clone();
    baseline.gate.strict = true;
    for policy in [
        &mut baseline.roles.source,
        &mut baseline.roles.test,
        &mut baseline.roles.generated,
        &mut baseline.roles.fixture,
        &mut baseline.roles.migration,
    ] {
        policy.severity = Some(Severity::Error);
    }
    baseline
}

struct LegacyBaselineRequest<'a> {
    config: &'a HardgateConfig,
    current: &'a mut GateReport,
    evidence: &'a ReferenceEvidence,
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
        "legacy ratchet: reference=`{}` merge-base=`{}` grandfathered={} retained={}; verdict covers new or worsened blocking static findings in the selected current scope, not a debt-free repository. Enabled current evidence is still required.",
        summary.reference, summary.merge_base, summary.grandfathered, summary.retained,
    ));
}

/// Inputs for filtering changed-line coverage to the current selected source.
pub(crate) struct ChangedLineFilter<'a> {
    pub changed_lines: &'a ChangedLineMap,
    pub selected_files: &'a [PathBuf],
    pub read_results: &'a [SharedSource],
    pub ownership: Option<&'a crate::discovery::rust_ownership::RustOwnership>,
    pub config: &'a HardgateConfig,
    pub root: &'a Path,
}

/// Keep only changed lines belonging to successfully read, selected,
/// AST-supported Source-role files.
pub(crate) fn filter_changed_lines(request: ChangedLineFilter<'_>) -> Result<ChangedLineMap> {
    let selected: BTreeSet<String> = request
        .selected_files
        .iter()
        .filter_map(|path| normalized_repository_key(path, request.root))
        .collect();
    let mut source_files = BTreeSet::new();
    let mut source_contents = std::collections::BTreeMap::new();
    let paths = request
        .read_results
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let classified = classify_files(&paths, request.config, request.root)?;
    let inputs = classified
        .iter()
        .zip(request.read_results)
        .map(|(file, (_, text))| (file, text.as_ref()))
        .collect::<Vec<_>>();
    let captured_ownership;
    let ownership = match request.ownership {
        Some(ownership) => ownership,
        None => {
            captured_ownership =
                crate::discovery::rust_ownership::RustOwnership::from_inputs(&inputs);
            &captured_ownership
        }
    };
    for ((path, content), classified) in request.read_results.iter().zip(classified) {
        let Some(key) = normalized_repository_key(path, request.root) else {
            continue;
        };
        if !selected.contains(&key) {
            continue;
        }
        if classified.ast_supported && ownership.file_role(&classified) == FileRole::Source {
            source_files.insert(key.clone());
            if let Some(view) = ownership
                .views(&classified, content)
                .into_iter()
                .find(|view| view.file.role == FileRole::Source)
            {
                source_contents.entry(key).or_insert(view.text);
            }
        }
    }

    Ok(request
        .changed_lines
        .iter()
        .filter_map(|(path, lines)| {
            let key = normalized_repository_key(path, request.root)?;
            if !source_files.contains(&key) {
                return None;
            }
            let filtered = source_contents
                .get(&key)
                .map(|content| retain_code_lines(content, lines))
                .unwrap_or_else(|| lines.clone());
            (!filtered.is_empty()).then_some((path.clone(), filtered))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{ChangedLineFilter, filter_changed_lines};
    use crate::config::HardgateConfig;
    use crate::git_evidence::ChangedLineMap;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    fn filtered(
        changed_lines: &ChangedLineMap,
        selected_files: &[PathBuf],
        read_results: &[(PathBuf, String)],
    ) -> ChangedLineMap {
        let shared = read_results
            .iter()
            .map(|(path, text)| (path.clone(), std::sync::Arc::from(text.as_str())))
            .collect::<Vec<_>>();
        filter_changed_lines(ChangedLineFilter {
            changed_lines,
            selected_files,
            read_results: &shared,
            ownership: None,
            config: &HardgateConfig::default(),
            root: Path::new("."),
        })
        .unwrap()
    }

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
        let filtered = filtered(&changed, &selected, &read);
        assert_eq!(
            filtered,
            BTreeMap::from([(PathBuf::from("src/lib.rs"), BTreeSet::from([1]))])
        );
    }

    #[test]
    fn changed_lines_drop_comments_and_delimiter_only_lines() {
        let changed = ChangedLineMap::from([(
            PathBuf::from("src/lib.rs"),
            BTreeSet::from([1, 2, 3, 4, 5, 6]),
        )]);
        let selected = vec![PathBuf::from("src/lib.rs")];
        let read = vec![(
            PathBuf::from("src/lib.rs"),
            "// comment\n}\nlet answer = 42;\n/* block */\nanswer += 1;\n}\n".to_string(),
        )];
        let filtered = filtered(&changed, &selected, &read);
        assert_eq!(
            filtered,
            BTreeMap::from([(PathBuf::from("src/lib.rs"), BTreeSet::from([3, 5]),)])
        );
    }
    #[test]
    fn changed_line_coverage_ignores_proven_inline_and_external_test_code() {
        let changed = ChangedLineMap::from([
            (PathBuf::from("src/lib.rs"), BTreeSet::from([1, 4])),
            (PathBuf::from("src/helper.rs"), BTreeSet::from([1])),
        ]);
        let selected = vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/helper.rs")];
        let read = vec![
            (
                selected[0].clone(),
                "pub fn source() {}\n#[cfg(test)]\nmod helper;\n#[test] fn check() {}\n"
                    .to_string(),
            ),
            (selected[1].clone(), "pub fn helper() {}\n".to_string()),
        ];
        assert_eq!(
            filtered(&changed, &selected, &read),
            ChangedLineMap::from([(PathBuf::from("src/lib.rs"), BTreeSet::from([1]))])
        );
    }
}
