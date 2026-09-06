use crate::config::ConfigContext;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Stable engine identifiers; names are independent of human diagnostic wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineId {
    FileBudgets,
    Suppressions,
    Complexity,
    Invariants,
    Clones,
    Coverage,
    MutationReport,
    GeneratedFreshness,
    LegacyRatchet,
    FormatCheck,
    Lint,
    Tests,
    Typecheck,
}

/// Cached is reserved for verified cache hits; current engines never emit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    Disabled,
    Skipped,
    Incomplete,
    Failed,
    Cached,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigIdentity {
    pub path: Option<PathBuf>,
    pub root: PathBuf,
    /// SHA-256 of the canonical, effective policy JSON, including explicit overrides.
    pub policy_sha256: String,
}

impl ConfigIdentity {
    pub fn from_context(context: &ConfigContext) -> serde_json::Result<Self> {
        // Keep the identity canonical even when a library consumer enables
        // serde_json's preserve_order feature through dependency unification.
        let mut effective = serde_json::to_value(&context.config)?;
        effective.sort_all_objects();
        let bytes = serde_json::to_vec(&effective)?;
        Ok(Self {
            path: context.config_path.clone(),
            root: context.root.clone(),
            policy_sha256: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionScope {
    pub mode: String,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineExecution {
    pub id: EngineId,
    pub enabled: bool,
    pub selected: bool,
    pub required_evidence: Vec<String>,
    pub state: EngineState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub command: String,
    pub scope: ExecutionScope,
    pub config: ConfigIdentity,
    pub engines: Vec<EngineExecution>,
}

impl ExecutionPlan {
    pub fn is_partial(&self) -> bool {
        self.command != "check"
            || self.scope.mode != "repository"
            || self
                .engines
                .iter()
                .any(|engine| engine.enabled && !engine.selected)
    }

    pub fn omitted_requirements(&self) -> Vec<EngineId> {
        self.engines
            .iter()
            .filter(|engine| engine.enabled && !engine.selected)
            .map(|engine| engine.id)
            .collect()
    }

    pub(crate) fn reconcile(
        &mut self,
        observations: &BTreeMap<EngineId, EngineState>,
        reasons: &BTreeMap<EngineId, String>,
    ) {
        for engine in &mut self.engines {
            if engine.selected
                && let Some(state) = observations.get(&engine.id)
            {
                engine.state = *state;
                engine.reason =
                    if *state == EngineState::Incomplete {
                        Some(reasons.get(&engine.id).cloned().unwrap_or_else(|| {
                            "required evidence could not be fully evaluated".into()
                        }))
                    } else {
                        None
                    };
            }
        }
    }
}

/// Observations aggregate without hiding failures from an earlier file/group.
pub(crate) fn observe(
    states: &mut BTreeMap<EngineId, EngineState>,
    id: EngineId,
    state: EngineState,
) {
    let current = states.entry(id).or_insert(state);
    if rank(state) > rank(*current) {
        *current = state;
    }
}

fn rank(state: EngineState) -> u8 {
    match state {
        EngineState::Disabled => 0,
        EngineState::Skipped => 1,
        EngineState::Cached => 2,
        EngineState::Completed => 3,
        EngineState::Failed => 4,
        EngineState::Incomplete => 5,
    }
}

pub(crate) fn evidence_engine(step: &str) -> EngineId {
    match step {
        "clone-index" | "read-clone-index" => EngineId::Clones,
        "coverage-report" | "coverage-diff" | "coverage-source-classification" => {
            EngineId::Coverage
        }
        "mutation-report" => EngineId::MutationReport,
        "generated-freshness" => EngineId::GeneratedFreshness,
        "legacy-ratchet" => EngineId::LegacyRatchet,
        _ => command_engine(step),
    }
}

fn command_engine(step: &str) -> EngineId {
    match step {
        "format_check" => EngineId::FormatCheck,
        "lint" => EngineId::Lint,
        "test" => EngineId::Tests,
        "typecheck" => EngineId::Typecheck,
        _ => EngineId::Complexity,
    }
}
