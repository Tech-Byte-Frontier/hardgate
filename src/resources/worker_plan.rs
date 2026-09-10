use super::{budget::reserve_bytes, memory, runtime::profile};
use std::io;

const MIB: u64 = 1024 * 1024;
const COORDINATOR_BYTES: u64 = 512 * MIB;

#[derive(Debug, serde::Serialize)]
pub(crate) struct WorkerPlan {
    pub jobs: usize,
    pub memory_limit_bytes: u64,
    pub available_bytes: u64,
    pub reserve_bytes: u64,
    pub coordinator_bytes: u64,
    pub worker_estimate_bytes: u64,
}

pub(crate) fn mutation_workers() -> io::Result<WorkerPlan> {
    let sample = memory::sample()?.ok_or_else(|| {
        super::runtime::error("mutation worker planning requires memory telemetry")
    })?;
    plan(&sample, profile::jobs(), profile::worker_memory_mib() * MIB)
}

fn plan(sample: &memory::MemorySample, jobs: usize, estimate: u64) -> io::Result<WorkerPlan> {
    let reserve = reserve_bytes(sample.total_bytes);
    sample.check(reserve)?;
    let capacity = sample
        .available_bytes
        .saturating_sub(reserve)
        .saturating_sub(COORDINATOR_BYTES);
    let workers = capacity.checked_div(estimate).unwrap_or(0).min(jobs as u64) as usize;
    if workers == 0 {
        return Err(super::runtime::error(format!(
            "no mutation runner fits: available {} MiB, reserve {} MiB, coordinator {} MiB, runner estimate {} MiB; select a larger --workload-memory-mib ceiling or a measured --mutation-worker-memory-mib estimate",
            sample.available_bytes / MIB,
            reserve / MIB,
            COORDINATOR_BYTES / MIB,
            estimate / MIB,
        )));
    }
    Ok(WorkerPlan {
        jobs: workers,
        memory_limit_bytes: sample.total_bytes,
        available_bytes: sample.available_bytes,
        reserve_bytes: reserve,
        coordinator_bytes: COORDINATOR_BYTES,
        worker_estimate_bytes: estimate,
    })
}

#[cfg(test)]
#[path = "worker_plan_tests.rs"]
mod tests;
