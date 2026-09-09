use super::{Admission, WorkloadGuard, error, profile};
use crate::resources::memory;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

mod supervisor;

const CHILD_MARKER: &str = "HARDGATE_WORKLOAD_CHILD";

pub(super) struct Boundary {
    directory: PathBuf,
    memory_limit: u64,
    jobs: usize,
    events: Vec<(PathBuf, String)>,
}

impl Boundary {
    pub(super) fn verify(&self) -> io::Result<()> {
        if !bounded(&self.directory, self.memory_limit, self.jobs)? {
            return Err(error("kernel limits changed during the workload"));
        }
        for (path, initial) in &self.events {
            let current = event_snapshot(path)?;
            if current != *initial {
                return Err(error(event_failure(
                    &self.directory,
                    path,
                    initial,
                    &current,
                )));
            }
        }
        Ok(())
    }
}

pub(super) fn enter() -> io::Result<Admission> {
    crate::cancellation::install()?;
    crate::resources::check_pressure()?;
    if let Some(inner) = find_boundary()? {
        supervisor::acknowledge()?;
        crate::engines::process::workload_status(
            "workload_start",
            &format!(
                "workload active: {}",
                resource_description(&inner.directory)?
            ),
            None,
        );
        let inner = std::sync::Arc::new(inner);
        super::ACTIVE
            .set(std::sync::Arc::clone(&inner))
            .map_err(|_| error("workload admission was already initialized"))?;
        return Ok(Admission::Current(WorkloadGuard { inner }));
    }
    if std::env::var_os(CHILD_MARKER).is_some() {
        return Err(error(
            "the supervisor did not establish verified CPU, memory, swap and task limits",
        ));
    }
    if std::env::var_os("HARDGATE_RESOURCE_SCRIPT_CHILD").is_some() {
        return Err(error(
            "the inherited maintenance boundary does not fit the selected allowance; cannot start a nested workload scope",
        ));
    }
    supervisor::run(memory_limit()?).map(Admission::Finished)
}

pub(super) fn memory_limit() -> io::Result<u64> {
    Ok(crate::resources::budget::align_memory_bytes(
        (memory::host_total_bytes()? / 4).min(profile::memory_ceiling(profile::jobs())),
    ))
}

pub(super) fn find_boundary() -> io::Result<Option<Boundary>> {
    let directories = memory::runtime_directories()?;
    let limit = memory_limit()?;
    let jobs = profile::jobs();
    for directory in directories {
        if bounded(&directory, limit, jobs)? {
            let events = ["memory.events", "pids.events"]
                .into_iter()
                .map(|name| {
                    let path = directory.join(name);
                    event_snapshot(&path).map(|contents| (path, contents))
                })
                .collect::<io::Result<Vec<_>>>()?;
            return Ok(Some(Boundary {
                directory,
                events,
                memory_limit: limit,
                jobs,
            }));
        }
    }
    Ok(None)
}

fn resource_description(directory: &Path) -> io::Result<String> {
    Ok(format!(
        "cpu.max={}, memory.max={} MiB, pids.max={} (processes and threads)",
        read(&directory.join("cpu.max"))?.trim(),
        numeric_limit(directory, "memory.max")?.unwrap_or(0) / (1024 * 1024),
        read(&directory.join("pids.max"))?.trim(),
    ))
}

fn event_failure(directory: &Path, path: &Path, initial: &str, current: &str) -> String {
    let kind = if path.ends_with("pids.events") {
        "task"
    } else {
        "memory"
    };
    let prefix = if kind == "task" { "pids" } else { "memory" };
    let usage = ["current", "peak", "max"]
        .map(|field| {
            let name = format!("{prefix}.{field}");
            let value = read(&directory.join(&name)).unwrap_or_else(|_| "unavailable".into());
            format!("{name}={}", value.trim())
        })
        .join(", ");
    format!(
        "{kind} limit events occurred; {usage}; counters [{}] -> [{}]; the evaluation is incomplete. Reduce child-tool concurrency or select a larger --workload-jobs allowance.",
        initial.trim().replace('\n', ", "),
        current.trim().replace('\n', ", ")
    )
}

fn event_snapshot(path: &Path) -> io::Result<String> {
    let value = read(path)?;
    let mut counters = std::collections::BTreeMap::new();
    for line in value.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 2 || fields[1].parse::<u64>().is_err() {
            return Err(error("invalid kernel resource event counter"));
        }
        if counters.insert(fields[0], fields[1]).is_some() {
            return Err(error("duplicate kernel resource event counter"));
        }
    }
    let required: &[&str] = if path.file_name().is_some_and(|name| name == "memory.events") {
        &["max", "oom", "oom_kill"]
    } else {
        &["max"]
    };
    if required.iter().any(|name| !counters.contains_key(name)) {
        return Err(error("missing kernel resource event counter"));
    }
    // Low/high and socket throttling are reclaim controls, not failed allocations.
    // Hard-limit events remain fail-closed; live PSI checks bound reclaim stalls.
    Ok(counters
        .into_iter()
        .filter(|(name, _)| !matches!(*name, "low" | "high" | "sock_throttled"))
        .map(|(name, value)| format!("{name} {value}\n"))
        .collect())
}

fn bounded(directory: &Path, memory_limit: u64, jobs: usize) -> io::Result<bool> {
    let cpu = read_optional(&directory.join("cpu.max"))?;
    let maximum = numeric_limit(directory, "memory.max")?;
    let high = numeric_limit(directory, "memory.high")?;
    let swap = numeric_limit(directory, "memory.swap.max")?;
    let tasks = numeric_limit(directory, "pids.max")?;
    Ok(cpu.as_deref().is_some_and(|value| cpu_bounded(value, jobs))
        && maximum.is_some_and(|value| value > 0 && value <= memory_limit)
        && high
            .zip(maximum)
            .is_some_and(|(high, max)| high > 0 && high <= max / 5 * 4)
        && swap == Some(0)
        && tasks.is_some_and(|value| value > 0 && value <= profile::task_limit(jobs)))
}

fn cpu_bounded(value: &str, jobs: usize) -> bool {
    let fields = value.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 2 {
        return false;
    }
    match (fields[0].parse::<u64>(), fields[1].parse::<u64>()) {
        (Ok(quota), Ok(period)) => {
            quota > 0 && period > 0 && quota as u128 <= period as u128 * jobs as u128
        }
        _ => false,
    }
}

fn numeric_limit(directory: &Path, name: &str) -> io::Result<Option<u64>> {
    read_optional(&directory.join(name))?
        .map(|value| {
            if value.trim() == "max" {
                return Ok(None);
            }
            value
                .trim()
                .parse()
                .map(Some)
                .map_err(|_| error(format!("invalid kernel limit {name}")))
        })
        .transpose()
        .map(Option::flatten)
}

fn read_optional(path: &Path) -> io::Result<Option<String>> {
    match read(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn read(path: &Path) -> io::Result<String> {
    let mut value = String::new();
    fs::File::open(path)?
        .take(8193)
        .read_to_string(&mut value)?;
    if value.len() > 8192 {
        return Err(error("oversized kernel resource evidence"));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;
