use super::memory::MemorySample;
use std::io;
use std::process::Command;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
const MEMORY_ALIGNMENT: u64 = 64 * 1024;

// cgroup memory controls round up to kernel pages. Align down first so the
// enforced readback never exceeds the intended byte ceiling (4/16/64 KiB pages).
pub(super) fn align_memory_bytes(bytes: u64) -> u64 {
    bytes / MEMORY_ALIGNMENT * MEMORY_ALIGNMENT
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MutationBudget {
    pub(crate) memory_bytes: u64,
    pub(crate) reserve_bytes: u64,
    pub(crate) jobs: usize,
}

pub(super) fn reserve_bytes(total: u64) -> u64 {
    (total / 10).clamp(256 * MIB, 2 * GIB)
}

impl MutationBudget {
    pub(crate) fn from_sample(sample: Option<&MemorySample>) -> io::Result<Self> {
        let (memory_bytes, reserve_bytes) = match sample {
            Some(sample) => {
                let reserve = reserve_bytes(sample.total_bytes);
                sample.check(reserve)?;
                let headroom = sample.available_bytes.saturating_sub(reserve);
                (
                    (headroom / 2).min(sample.total_bytes / 4).min(8 * GIB),
                    reserve,
                )
            }
            None => (2 * GIB, 256 * MIB),
        };
        let memory_bytes = align_memory_bytes(memory_bytes);
        if memory_bytes < 64 * MIB {
            return Err(io::Error::other(
                "mutation resource guard: insufficient memory headroom; close other workloads and retry",
            ));
        }
        let cpus = std::thread::available_parallelism().map_or(1, usize::from);
        Ok(Self {
            memory_bytes,
            reserve_bytes,
            jobs: cpus.min(super::runtime::profile::jobs()),
        })
    }

    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn high_bytes(self) -> u64 {
        Self::high_memory_bytes(self.memory_bytes)
    }

    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn high_memory_bytes(memory_bytes: u64) -> u64 {
        align_memory_bytes(memory_bytes / 5 * 4)
    }

    pub(crate) fn constrain_environment(self, command: &mut Command) {
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
            command.env(
                key,
                bounded_jobs(std::env::var(key).ok().as_deref(), self.jobs).to_string(),
            );
        }
        // An inherited Cargo jobserver describes file descriptors which are
        // not transferred to an independent mutation command.
        command.env_remove("CARGO_MAKEFLAGS");
        command.env("MAKEFLAGS", format!("-j{}", self.jobs));
        command.env("HARDGATE_MUTATION_CHILD", "1");
    }

    pub(crate) fn description(self, contained: bool) -> String {
        let boundary = if contained {
            format!(
                "Linux cgroup memory cap {} MiB, swap disabled",
                self.memory_bytes / MIB
            )
        } else {
            if cfg!(target_os = "linux") {
                "sampled memory checks; aggregate cgroup containment unavailable"
            } else {
                "memory telemetry and aggregate cgroup containment unavailable"
            }
            .to_string()
        };
        format!(
            "mutation resource guard: one workload per user, common build/test worker defaults capped at {}; {boundary}",
            self.jobs
        )
    }
}

fn bounded_jobs(inherited: Option<&str>, maximum: usize) -> usize {
    inherited
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .map_or(maximum, |value| value.min(maximum))
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
