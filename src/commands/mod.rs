pub use check::{
    CheckOptions, Emission, OutputOptions, cmd_check, emit_gate_report, output_report,
    output_report_with_opts, print_empty_discovery,
};
pub mod check;
mod evidence;
pub mod fmt;
pub mod init;
pub mod mutate;
mod mutation_output;
mod role_policy;
pub mod scan;
mod static_gate;
pub mod verify;

pub use fmt::cmd_fmt;
pub use init::cmd_init;
pub use mutate::{MutateOptions, cmd_mutate};
pub use mutation_output::{MutationSummaryContext, format_mutation_terminal};
pub use scan::{cmd_scan, cmd_scan_with_format};
pub use static_gate::{
    AnalyzeInput, StaticGateOutcome, StaticSnapshotOutcome, analyze_file_content, run_static_gate,
    run_static_gate_scoped, run_static_gate_snapshot,
};
pub use verify::{VerifyOptions, cmd_verify, cmd_verify_legacy};
