pub mod commands;
pub mod config;
pub mod diagnostics;
pub mod discovery;
pub mod engines;
pub mod mcp;

pub use config::{HardgateConfig, Preset};
pub use diagnostics::GateReport;
pub use discovery::{
    discover_files, discover_files_with_exclusions, DiscoverOptions, DiscoveryResult,
};
