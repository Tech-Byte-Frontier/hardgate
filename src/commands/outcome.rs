use crate::diagnostics::GateReport;
use std::io::{self, BufWriter, Write};

#[path = "file_output.rs"]
mod file_output;
pub(crate) use file_output::write_atomic_file;

/// Process-independent result: main alone chooses the process exit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    Passed,
    Violations,
    Incomplete,
}

pub type CommandResult = anyhow::Result<CommandOutcome>;

impl CommandOutcome {
    pub fn status(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Violations => "violations",
            Self::Incomplete => "incomplete",
        }
    }

    pub fn exit_code(self) -> u8 {
        match self {
            Self::Passed => 0,
            Self::Violations => 1,
            Self::Incomplete => 2,
        }
    }

    pub fn from_report(report: &GateReport) -> Self {
        if incomplete_evidence(report) {
            Self::Incomplete
        } else if report.passed {
            Self::Passed
        } else {
            Self::Violations
        }
    }
}

fn incomplete_evidence(report: &GateReport) -> bool {
    report
        .orchestration_violations
        .iter()
        .any(|failure| failure.exit_code.is_none() || matches!(failure.exit_code, Some(126 | 127)))
        || report.coverage_violations.iter().any(|failure| {
            crate::diagnostics::execution_observations::missing_coverage(&failure.metric)
        })
}

pub(crate) fn write_stdout(text: &str) -> io::Result<()> {
    crate::resources::runtime::verify_active()?;
    let mut output = BufWriter::new(io::stdout().lock());
    output.write_all(text.as_bytes())?;
    output.flush()
}

/// A downstream consumer intentionally closing stdout is successful termination.
pub fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
    })
}

pub(crate) fn append_scan_metrics(out: &mut String, functions: &[crate::engines::FunctionMetrics]) {
    if functions.is_empty() {
        return;
    }
    out.push_str("\nFunction metrics (all analyzed functions):\n");
    for function in functions {
        out.push_str(&format!(
            "  {}:{} {}: cyclomatic={}, parameters={}, lines={}, nesting={}, statements={}\n",
            function.file.display(),
            function.start_line,
            function.name,
            function.cyclomatic,
            function.parameters,
            function.lines,
            function.max_nesting_depth,
            function.statements,
        ));
    }
}
