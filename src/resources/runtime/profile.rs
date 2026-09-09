use super::error;
use std::io;
use std::sync::OnceLock;

pub(crate) const JOBS_ENV: &str = "HARDGATE_WORKLOAD_JOBS";
const MAX_JOBS: usize = 64;
static JOBS: OnceLock<usize> = OnceLock::new();

/// Resolve the workload allowance before admission; descendants inherit it.
pub fn configure(requested: Option<usize>) -> io::Result<()> {
    let inherited = match requested {
        Some(_) => None,
        None => match std::env::var(JOBS_ENV) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err(error("HARDGATE_WORKLOAD_JOBS must be a UTF-8 integer")),
        },
    };
    let jobs = select(requested, inherited.as_deref(), available())?;
    JOBS.set(jobs)
        .map_err(|_| error("workload jobs were already configured"))
}

fn available() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

pub(crate) fn jobs() -> usize {
    JOBS.get().copied().unwrap_or_else(|| {
        select(None, std::env::var(JOBS_ENV).ok().as_deref(), available()).unwrap_or(1)
    })
}

fn default_jobs(cpus: usize) -> usize {
    (cpus / 2).clamp(1, 8)
}

fn select(requested: Option<usize>, inherited: Option<&str>, cpus: usize) -> io::Result<usize> {
    let inherited = inherited
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| error("HARDGATE_WORKLOAD_JOBS must be an integer between 1 and 64"))
        })
        .transpose()?;
    let jobs = requested
        .or(inherited)
        .unwrap_or_else(|| default_jobs(cpus));
    if !(1..=MAX_JOBS).contains(&jobs) {
        return Err(error(
            "--workload-jobs / HARDGATE_WORKLOAD_JOBS must be between 1 and 64",
        ));
    }
    Ok(jobs)
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn task_limit(jobs: usize) -> u64 {
    (jobs as u64 * 128).max(256)
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn memory_ceiling(jobs: usize) -> u64 {
    (jobs as u64).clamp(4, 16) * 1024 * 1024 * 1024
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
