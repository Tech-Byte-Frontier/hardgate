use super::error;
use std::io;
use std::sync::OnceLock;

pub(crate) const JOBS_ENV: &str = "HARDGATE_WORKLOAD_JOBS";
const MAX_JOBS: usize = 64;
static JOBS: OnceLock<usize> = OnceLock::new();
pub(crate) const MEMORY_ENV: &str = "HARDGATE_WORKLOAD_MEMORY_MIB";
pub(crate) const WORKER_MEMORY_ENV: &str = "HARDGATE_MUTATION_WORKER_MEMORY_MIB";
static MEMORY: OnceLock<Option<u64>> = OnceLock::new();
static WORKER_MEMORY: OnceLock<u64> = OnceLock::new();

/// Configure bounded memory independently from the CPU allowance.
pub fn configure_memory(memory: Option<u64>, worker: Option<u64>) -> io::Result<()> {
    let memory = memory_setting(memory, MEMORY_ENV, 1024..=65536)?;
    let worker = memory_setting(worker, WORKER_MEMORY_ENV, 1024..=16384)?.unwrap_or(2048);
    MEMORY
        .set(memory)
        .map_err(|_| error("workload memory was already configured"))?;
    WORKER_MEMORY
        .set(worker)
        .map_err(|_| error("worker memory was already configured"))
}

fn memory_setting(
    requested: Option<u64>,
    key: &str,
    range: std::ops::RangeInclusive<u64>,
) -> io::Result<Option<u64>> {
    let value = match requested {
        Some(value) => Some(value),
        None => match std::env::var(key) {
            Ok(value) => Some(
                value
                    .parse()
                    .map_err(|_| error(format!("{key} must be an integer")))?,
            ),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err(error(format!("{key} must be UTF-8"))),
        },
    };
    if value.is_some_and(|value| !range.contains(&value)) {
        return Err(error(format!(
            "{key} must be between {} and {} MiB",
            range.start(),
            range.end()
        )));
    }
    Ok(value)
}

pub(crate) fn requested_memory_mib() -> Option<u64> {
    MEMORY.get().copied().flatten()
}

pub(crate) fn worker_memory_mib() -> u64 {
    WORKER_MEMORY.get().copied().unwrap_or(2048)
}

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
    (cpus / 2).clamp(1, MAX_JOBS)
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
    requested_memory_mib().unwrap_or((jobs as u64).clamp(4, 16) * 1024) * 1024 * 1024
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
