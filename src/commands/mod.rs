pub use check::{
    CheckOptions, Emission, OutputOptions, cmd_check, emit_gate_report, output_report,
    output_report_with_opts, print_empty_discovery, run_static_gate, run_static_gate_scoped,
};
pub mod check;
pub mod fmt;
pub mod init;
pub mod mutate;
pub mod scan;
pub mod verify;

pub use fmt::cmd_fmt;
pub use init::cmd_init;
pub use mutate::{MutateOptions, MutationSummaryContext, cmd_mutate, format_mutation_terminal};
pub use scan::{cmd_scan, cmd_scan_with_format};
pub use verify::{VerifyOptions, cmd_verify, cmd_verify_legacy};
