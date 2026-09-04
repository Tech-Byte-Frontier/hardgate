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
//!   linter, and test commands with bounded process handling.
//!
//! ## Roles and evidence
//!
//! Source, test, generated, fixture, and migration roles have independent
//! severity, budget, and clone policies. Native mutation is source-role-only,
//! with source eligibility configurable; other roles remain ineligible.
//! Generated freshness remains a separate current check even when generated
//! paths are excluded from file budgets. Each enabled coverage, mutation,
//! freshness, and legacy reference check is required and blocking; empty or
//! missing evidence is never treated as a pass. A legacy reference/merge-base
//! ratchet may grandfather non-worsened static and configured dead-code debt,
//! while current evidence engines remain outside the ratchet. Under that
//! ratchet, new or worsened findings with effective role severity `error` block;
//! `warning` findings are advisories and `ignore` findings are omitted.
//!
//! The JavaScript-family parser set includes `.js`, `.jsx`, `.mjs`, `.cjs`,
//! `.ts`, `.tsx`, `.mts`, and `.cts`. Built-in Supabase conventions classify
//! `supabase/database.types.ts` and `supabase/schema.gen.ts` as generated,
//! `supabase/functions/**/*.ts` as source, and migration/seed SQL as
//! migration. SQL migration/seed files remain inventoried for applicable
//! safety policy but have no AST parser; `supabase/seed.ts` is migration-role
//! and has TypeScript parser support, while migration policy does not apply
//! ordinary source/test complexity or native mutation. Under the default
//! strict migration policy, parser-unsupported migration files produce a
//! blocking `unsupported-source` finding. A custom classification rule may
//! assign another role, but it does not add a SQL parser.
//!
//! ## Command boundaries
//!
//! `check` runs static engines plus enabled report and freshness evaluators.
//! `check --diff` selects Git-changed/staged inventory by default, adds
//! explicit existing paths to static/clone selection, compares clones against
//! a full repository index, and evaluates LCOV only on actual changed
//! executable lines; an enabled legacy ratchet disables diff filtering but
//! honors explicit path filters, using the full current selected scope (the
//! whole tree when no paths are supplied) for static/clone comparison with
//! changed-hunk attribution. `check --all`
//! additionally runs configured formatter, linter, and test commands. `verify`
//! runs full static/dead-code and configured evidence by default; optional path
//! filters scope only current static/dead-code inventory and coverage source
//! matching while mutation reports and freshness remain configured/full. The
//! ratchet still loads and validates the full configured reference snapshot,
//! then compares it only to selected current static/dead-code findings without
//! widening explicit paths.
//! It does not run orchestration or native mutation. When enabled, `mutate`
//! runs the native unmutated baseline and AST mutants; when disabled, it emits
//! a note and succeeds without target discovery or execution.
//! Native mutation is supported only on the six Linux/macOS release target
//! families (Linux x64/arm64 glibc and musl, macOS x64/arm64); all other
//! targets, including other Unix systems, fail closed before baseline or
//! source writes.
//! After an explicit scope is validated, a `mutate --diff` run (including
//! `--scoped`) with no changed production source is a successful no-op. Missing,
//! invalid, unsupported, or non-source explicit scopes fail closed; only a
//! non-diff unrestricted or scoped run with no eligible source target fails.
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
//! static gate; `diff` selects Git-changed/staged inventory by default, and
//! explicit existing paths add to static/clone selection with full-index clone
//! matching. MCP never runs
//! coverage, mutation, freshness, dead-code, orchestration, or native
//! mutation. Invalid arguments/configuration, missing paths, empty
//! scopes/discovery, and Git failures are outer tool errors. Read/parse
//! failures remain report-level Hardgate `Failed` findings whose effective
//! role severity makes them errors, advisories, or omitted findings (`error`,
//! `warning`, or `ignore`). For `hardgate_scan_file`, a read failure is an
//! outer tool error while parse/static findings remain in its per-file report;
//! `hardgate_get_metrics` reports read or missing-symbol errors explicitly.
//!
//! ## Build identity
//!
//! Release metadata binds the binary name, numeric version, Cargo target
//! triple, npm package, and full source commit in `BUILD-METADATA.json`.
//! Binaries embed `hardgate-target:<target>` and report exactly
//! `hardgate VERSION (COMMIT)` for `--version`; release verification checks
//! the checksum, metadata, target marker, and version/commit identity together.

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
