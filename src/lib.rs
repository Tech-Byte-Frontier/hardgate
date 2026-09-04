//! # Hardgate
//!
//! **Deterministic, policy-driven quality checks for structural budgets, anti-gaming rules, and
//! local verification evidence.**
//!
//! A run discovers inventory files, classifies repository roles, and applies the engines enabled by
//! `hardgate.toml`.
//!
//! ## Engines
//!
//! * **[`engines::complexity`]** parses Rust (`.rs`), TypeScript (`.ts`, `.mts`, `.cts`, `.tsx`),
//!   JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`), Python (`.py`), and Go (`.go`) with Tree-sitter.
//!   It reports cyclomatic, cognitive, Halstead, ABC, parameter, line, statement, and nesting
//!   metrics for functions.
//! * **[`engines::anti_gaming`]** reports configured suppression directives and project-specific
//!   forbidden tokens; strict defaults disallow the built-in suppression patterns.
//! * **[`engines::budgets`]** enforces configured file byte/line limits and per-function ceilings.
//! * **[`engines::clones`]** compares token streams with a rolling hash and reports matching spans,
//!   honoring configured thresholds and exclusions.
//! * **[`engines::invariants`]** checks path-scoped rules for forbidden imports, calls, and tokens.
//! * **[`engines::coverage`]** reads LCOV reports and evaluates configured line, function, and branch
//!   floors, CRAP scores, and critical paths.
//! * **[`engines::mutation`]** generates and runs native AST mutations for supported production
//!   files, using resolved test commands, per-mutant timeouts, and rollback guards; it also
//!   evaluates configured mutation reports.
//! * **[`engines::dead_code`]** optionally reports unreferenced files and JavaScript/TypeScript
//!   exports.
//! * **[`engines::orchestration`]** optionally runs configured formatter, linter, and test commands.
//!
//! ## CLI behavior
//!
//! `hardgate check` starts with the static engines. `check --all` additionally runs configured
//! formatter, linter, and test commands. Dead-code analysis can be requested with `--dead-code` or
//! enabled in configuration; coverage and mutation report checks run when their policies are enabled.
//! `hardgate verify` runs the static engines and evaluates enabled coverage and mutation reports.
//! Native mutation execution is a separate `hardgate mutate` command.
//!
//! ## Example: Loading Configuration and Inspecting a Project
//!
//! ```no_run
//! use hardgate::config::HardgateConfig;
//! use hardgate::discovery::{discover_files, DiscoverOptions};
//! use std::path::Path;
//!
//! // Load hardgate.toml or the strict-agent defaults.
//! let config = HardgateConfig::load_or_default(None).expect("config load");
//!
//! // Discover inventory files; budget exclusions remain visible to other engines.
//! let root = Path::new(".");
//! let files = discover_files(DiscoverOptions {
//!     root,
//!     diff_only: false,
//!     exclusions: &config.budgets.files.exclusions.paths,
//! }).expect("file discovery");
//!
//! println!("Discovered {} inventory files for analysis.", files.len());
//! ```
//!
//! ## Model Context Protocol (MCP) server
//!
//! `hardgate mcp` serves MCP over standard input/output. It exposes the following tools:
//!
//! * `hardgate_check` checks the repository or supplied paths.
//! * `hardgate_scan_file` analyzes one file.
//! * `hardgate_get_metrics` returns metrics for a function symbol in a file.

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
