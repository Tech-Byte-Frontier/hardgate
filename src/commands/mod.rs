pub use check::{
    CheckOptions, Emission, OutputOptions, cmd_check, cmd_check_in, emit_gate_report,
    output_report, output_report_with_opts, print_empty_discovery,
};
pub mod check;
mod check_selection;
pub use check_selection::CheckKind;
mod evidence;
mod execution_failure;
pub(crate) mod execution_plan;
pub use execution_failure::ExecutionFailure;
pub mod fmt;
mod gate_evidence;
pub mod init;
pub mod inspect;
pub mod outcome;
pub use outcome::{CommandOutcome, CommandResult};
pub mod report;
mod role_policy;
pub mod scan;
mod source_snapshot;
mod static_gate;
pub mod verify;

pub use report::{ReportInspectOptions, cmd_report_compare, cmd_report_inspect};

pub use fmt::{cmd_fmt, cmd_fmt_in};
pub use init::cmd_init;
pub use scan::{cmd_scan, cmd_scan_in, cmd_scan_with_format};
pub use static_gate::{
    AnalyzeInput, StaticGateOutcome, StaticSnapshotOutcome, analyze_file_content, run_static_gate,
    run_static_gate_at, run_static_gate_scoped, run_static_gate_snapshot,
};

pub(crate) use static_gate::{StaticRequest, run_shared_gate};
mod specialist;
