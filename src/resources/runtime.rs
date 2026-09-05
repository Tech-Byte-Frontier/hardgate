//! Resource admission for complete CLI workloads, including their descendants.
use std::io;
use std::process::Command;

#[cfg(target_os = "linux")]
mod linux;

/// Maximum default analysis and build parallelism. Smaller settings are retained.
pub fn worker_limit(requested: Option<usize>) -> io::Result<usize> {
    let maximum = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(2);
    let inherited = std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|value| value.parse().ok());
    select_workers(requested, inherited, maximum)
}

fn select_workers(
    requested: Option<usize>,
    inherited: Option<usize>,
    maximum: usize,
) -> io::Result<usize> {
    if requested.is_some_and(|count| count == 0 || count > maximum) {
        return Err(error(format!(
            "--threads must be between 1 and {maximum}; the resource boundary remains enforced"
        )));
    }
    Ok(requested
        .or(inherited.filter(|count| *count > 0))
        .unwrap_or(maximum)
        .min(maximum))
}

pub(crate) fn error(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("workload resource guard: {message}"))
}

/// Apply conservative child-tool defaults; OS containment remains authoritative.
pub(crate) fn constrain_command(command: &mut Command) {
    for key in [
        "CARGO_BUILD_JOBS",
        "RUST_TEST_THREADS",
        "RAYON_NUM_THREADS",
        "GOMAXPROCS",
        "OMP_NUM_THREADS",
        "OPENBLAS_NUM_THREADS",
        "MKL_NUM_THREADS",
        "CMAKE_BUILD_PARALLEL_LEVEL",
    ] {
        let count = std::env::var(key)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|count| *count > 0)
            .unwrap_or(2)
            .min(2);
        command.env(key, count.to_string());
    }
    command
        .env_remove("CARGO_MAKEFLAGS")
        .env("MAKEFLAGS", "-j2");
}

/// A supervised exit, or admission of the current process into a verified boundary.
pub enum Admission {
    /// The supervised invocation has finished with this CLI exit status.
    Finished(u8),
    /// The current process and its children already share enforced limits.
    Current(WorkloadGuard),
}

/// Retains kernel evidence for the current invocation.
pub struct WorkloadGuard {
    #[cfg(target_os = "linux")]
    inner: std::sync::Arc<linux::Boundary>,
}

impl WorkloadGuard {
    /// Reject resource-limit events before accepting a command outcome.
    pub fn verify(&self) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        return self.inner.verify();
        #[cfg(not(target_os = "linux"))]
        Err(error(
            "OS-level workload containment is unavailable on this platform",
        ))
    }
}

/// Enforce the CLI-wide resource boundary before reading project input or starting tools.
pub fn enter() -> io::Result<Admission> {
    #[cfg(target_os = "linux")]
    return linux::enter();
    #[cfg(not(target_os = "linux"))]
    Err(error(
        "this platform has no enforced workload backend; analysis and external tools were not started",
    ))
}

/// Verify inherited containment before launching a project command.
pub(crate) fn inherited() -> io::Result<bool> {
    #[cfg(target_os = "linux")]
    return Ok(linux::find_boundary()?.is_some());
    #[cfg(not(target_os = "linux"))]
    Ok(false)
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

/// Check active kernel evidence before publishing a report or saved output.
pub(crate) fn verify_active() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    if let Some(boundary) = ACTIVE.get() {
        boundary.verify()?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
static ACTIVE: std::sync::OnceLock<std::sync::Arc<linux::Boundary>> = std::sync::OnceLock::new();
