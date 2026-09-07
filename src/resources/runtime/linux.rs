use super::{Admission, WorkloadGuard, error};
use crate::resources::memory;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

mod supervisor;

const GIB: u64 = 1024 * 1024 * 1024;
const MAX_MEMORY: u64 = 4 * GIB;
const MAX_TASKS: u64 = 256;
const CHILD_MARKER: &str = "HARDGATE_WORKLOAD_CHILD";

pub(super) struct Boundary {
    directory: PathBuf,
    events: Vec<(PathBuf, String)>,
}

impl Boundary {
    pub(super) fn verify(&self) -> io::Result<()> {
        if !bounded(&self.directory, memory_limit()?)? {
            return Err(error("kernel limits changed during the workload"));
        }
        for (path, initial) in &self.events {
            if event_snapshot(path)? != *initial {
                return Err(error(
                    "memory or task limit events occurred; the evaluation is incomplete",
                ));
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
    supervisor::run(memory_limit()?).map(Admission::Finished)
}

pub(super) fn memory_limit() -> io::Result<u64> {
    Ok(crate::resources::budget::align_memory_bytes(
        (memory::host_total_bytes()? / 4).min(MAX_MEMORY),
    ))
}

pub(super) fn find_boundary() -> io::Result<Option<Boundary>> {
    let directories = memory::runtime_directories()?;
    let limit = memory_limit()?;
    for directory in directories {
        if bounded(&directory, limit)? {
            let events = ["memory.events", "pids.events"]
                .into_iter()
                .map(|name| {
                    let path = directory.join(name);
                    event_snapshot(&path).map(|contents| (path, contents))
                })
                .collect::<io::Result<Vec<_>>>()?;
            return Ok(Some(Boundary { directory, events }));
        }
    }
    Ok(None)
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

fn bounded(directory: &Path, memory_limit: u64) -> io::Result<bool> {
    let cpu = read_optional(&directory.join("cpu.max"))?;
    let maximum = numeric_limit(directory, "memory.max")?;
    let high = numeric_limit(directory, "memory.high")?;
    let swap = numeric_limit(directory, "memory.swap.max")?;
    let tasks = numeric_limit(directory, "pids.max")?;
    Ok(cpu.as_deref().is_some_and(cpu_bounded)
        && maximum.is_some_and(|value| value > 0 && value <= memory_limit)
        && high
            .zip(maximum)
            .is_some_and(|(high, max)| high > 0 && high <= max / 5 * 4)
        && swap == Some(0)
        && tasks.is_some_and(|value| value > 0 && value <= MAX_TASKS))
}

fn cpu_bounded(value: &str) -> bool {
    let fields = value.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 2 {
        return false;
    }
    match (fields[0].parse::<u64>(), fields[1].parse::<u64>()) {
        (Ok(quota), Ok(period)) => quota > 0 && period > 0 && quota as u128 <= period as u128 * 2,
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
