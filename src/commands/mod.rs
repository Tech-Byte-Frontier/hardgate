pub use check::{
    CheckOptions, Emission, OutputOptions, cmd_check, cmd_check_in, emit_gate_report,
    output_report, output_report_with_opts, print_empty_discovery,
};
pub mod check;
mod dead_code;
mod evidence;
mod execution_failure;
pub(crate) mod execution_plan;
pub use execution_failure::ExecutionFailure;
pub mod fmt;
mod gate_evidence;
pub mod init;
pub mod inspect;
pub mod mutate;
mod mutation_output;
pub mod outcome;
pub use outcome::{CommandOutcome, CommandResult};
mod role_policy;
pub mod scan;
mod source_snapshot;
mod static_gate;
pub mod verify;

pub use fmt::{cmd_fmt, cmd_fmt_in};
pub use init::cmd_init;
pub use mutate::{MutateOptions, cmd_mutate, cmd_mutate_in};
pub use mutation_output::{MutationFailure, MutationSummaryContext, format_mutation_terminal};
pub use scan::{cmd_scan, cmd_scan_in, cmd_scan_with_format};
pub use static_gate::{
    AnalyzeInput, StaticGateOutcome, StaticSnapshotOutcome, analyze_file_content, run_static_gate,
    run_static_gate_at, run_static_gate_scoped, run_static_gate_snapshot,
};
pub use verify::{VerifyOptions, cmd_verify, cmd_verify_in, cmd_verify_legacy};

pub(crate) use static_gate::{StaticRequest, run_shared_gate};
