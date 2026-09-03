//! # Hardgate
//!
//! **Deterministic quality gates, hard budgets, and anti-gaming verification harness for the AI agent era.**
//!
//! Hardgate evaluates physical file budgets, Tree-sitter AST complexity metrics (cyclomatic,
//! cognitive, Halstead, ABC), suppression pragmas, duplicate code clones, architectural invariants,
//! dead code, and native mutation testing in **under 200 milliseconds** — and in **under 10ms on git diffs**.
//!
//! ## Core Architecture & Engines
//!
//! * **[`engines::complexity`]**: Multi-language Tree-sitter AST parser computing Cyclomatic,
//!   Cognitive, Halstead difficulty, and ABC complexity metrics with per-node diagnostic contributors.
//! * **[`engines::anti_gaming`]**: Zero-tolerance suppression detection across Rust, TypeScript,
//!   JavaScript, Python, and Go (rejects pragma suppressions like `ts-ignore`, `allow(...)`, `noqa`, etc.).
//! * **[`engines::clones`]**: High-performance Rabin-Karp token-stream clone detector identifying
//!   duplicate logic blocks.
//! * **[`engines::invariants`]**: Declarative architectural boundary enforcement forbidding illegal
//!   cross-module dependencies and calls.
//! * **[`engines::mutation`]**: Native Tree-Sitter AST mutation testing runner with RAII rollback guards.
//! * **[`engines::dead_code`]**: Dead code and unreferenced export analysis.
//! * **Technical Debt Advisories**: Emits non-blocking advisory warnings when files are excluded from
//!   clone detection or file budget checks.
//!
//! ## Example: Loading Configuration and Inspecting a Project
//!
//! ```no_run
//! use hardgate::config::HardgateConfig;
//! use hardgate::discovery::{discover_files, DiscoverOptions};
//! use std::path::Path;
//!
//! // Load configuration or fallback to strict agent preset
//! let config = HardgateConfig::load_or_default(None).expect("config load");
//!
//! // Discover source files matching configured exclusions
//! let root = Path::new(".");
//! let files = discover_files(DiscoverOptions {
//!     root,
//!     diff_only: false,
//!     exclusions: &config.budgets.files.exclusions.paths,
//! }).expect("file discovery");
//!
//! println!("Discovered {} source files for analysis.", files.len());
//! ```
//!
//! ## Model Context Protocol (MCP) Server
//!
//! Hardgate includes an embedded MCP server (`hardgate mcp`) exposing static analysis tools
//! (`hardgate_check`, `hardgate_scan_file`, `hardgate_get_metrics`) over standard I/O for
//! Claude Code, Cursor, Windsurf, and Cline.

pub mod commands;
pub mod config;
pub mod diagnostics;
pub mod discovery;
pub mod engines;
pub mod mcp;

pub use config::{HardgateConfig, Preset};
pub use diagnostics::{GateReport, GateSummary, TopFileEntry};
pub use discovery::{
    DiscoverOptions, DiscoveryResult, discover_files, discover_files_with_exclusions,
    filter_files_by_paths,
};
