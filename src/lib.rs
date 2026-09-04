//! # Hardgate
//!
//! **Deterministic, policy-driven quality checks for structural budgets,
//! anti-gaming rules, and local verification evidence.**
//!
//! Hardgate inventories repository files, classifies their roles, applies the
//! engines enabled by `hardgate.toml`, and renders one report for people,
//! agents, or automation. A missing configuration uses the same `strict-agent`
//! preset object rendered by `hardgate init --preset strict-agent`.
//!
//! ## Engines
//!
//! * **[`engines::complexity`]** parses Rust, JavaScript, TypeScript/TSX,
//!   Python, and Go with Tree-sitter and reports function metrics.
//! * **[`engines::anti_gaming`]** finds configured suppression directives and
//!   forbidden tokens in safety-checked roles.
//! * **[`engines::budgets`]** enforces file byte/line and function ceilings.
//! * **[`engines::clones`]** compares normalized token windows in independent
//!   source, test, and fixture groups. Clone fingerprints exclude paths and
//!   line numbers so safe rename lineage can preserve identity.
//! * **[`engines::invariants`]** checks declarative path-scoped import, call,
//!   and token rules.
//! * **[`engines::coverage`]** evaluates enabled LCOV floors, CRAP scores,
//!   critical paths, and changed executable lines in diff mode.
//! * **[`engines::mutation`]** evaluates configured mutation reports and runs
//!   native AST mutation through the `mutate` command.
//! * **[`engines::dead_code`]** optionally reports unreferenced files and
//!   JavaScript/TypeScript exports.
//! * **[`engines::generated`]** runs an enabled generated-artifact freshness
//!   command independently of file-budget exclusions.
//! * **[`engines::orchestration`]** runs explicitly configured formatter,
//!   linter, test, and freshness commands with bounded process handling.
//!
//! ## Roles and evidence
//!
//! Source, test, generated, fixture, and migration roles have independent
//! severity, budget, clone, and native-mutation policies. Generated freshness
//! remains a separate current check even when generated paths are excluded
//! from file budgets. Each enabled coverage, mutation, freshness, and legacy
//! reference check is required and blocking; empty or missing evidence is
//! never treated as a pass. A legacy reference/merge-base ratchet may
//! grandfather non-worsened static and configured dead-code debt, while
//! current evidence engines remain outside the ratchet.
//!
//! ## Command boundaries
//!
//! `check` runs static engines plus enabled report and freshness evaluators.
//! `check --diff` scopes static files to changed/staged inventory, compares
//! clones against a full repository index, and evaluates changed executable
//! LCOV lines; an enabled legacy ratchet performs a full-tree static comparison
//! with changed-hunk attribution. `check --all` additionally runs configured
//! formatter, linter, and test commands. `verify` runs full static analysis,
//! enabled reports/freshness, and the legacy static ratchet without
//! orchestration or native mutation. `mutate` runs the native unmutated
//! baseline and AST mutants.
//!
//! ## Example: loading configuration and discovering files
//!
//! ```no_run
//! use hardgate::config::HardgateConfig;
//! use hardgate::discovery::{DiscoverOptions, discover_files};
//! use std::path::Path;
//!
//! let config = HardgateConfig::load_or_default(None).expect("config load");
//! let files = discover_files(DiscoverOptions {
//!     root: Path::new("."),
//!     diff_only: false,
//!     exclusions: &config.budgets.files.exclusions.paths,
//! })
//! .expect("file discovery");
//! println!("Discovered {} inventory files.", files.len());
//! ```
//!
//! ## Model Context Protocol (MCP)
//!
//! `hardgate mcp` serves MCP over stdio. The static-only tools are
//! `hardgate_check(paths?, diff?)`, `hardgate_scan_file(path)`, and
//! `hardgate_get_metrics(path, symbol)`. `hardgate_check` routes through the
//! static gate and fails closed on invalid configuration or arguments, empty
//! scopes/discovery, unreadable or unparsable files, and Git failures. It does
//! not run coverage, mutation, freshness, dead-code, orchestration, or native
//! mutation.

pub mod adoption;
pub mod commands;
pub mod config;
pub mod diagnostics;
pub mod discovery;
pub mod engines;
pub mod git_evidence;
pub mod mcp;

pub use adoption::{
    LegacyRatchetOutcome, LegacyRatchetSummary, apply_legacy_ratchet, ratchet_report,
};
pub use config::{HardgateConfig, Preset};
pub use diagnostics::{GateReport, GateSummary, TopFileEntry};
pub use discovery::{
    ClassifiedFile, DiscoverOptions, DiscoveryResult, FileRole, discover_files,
    discover_files_with_exclusions, filter_files_by_paths,
};
pub use git_evidence::{
    ChangeSet, ChangedLineMap, GitEvidence, ReferenceEvidence, RepositorySnapshot, load_reference,
    touches,
};
