use std::io;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MemorySample {
    pub(crate) total_bytes: u64,
    pub(crate) available_bytes: u64,
    pub(crate) full_avg10: f64,
    pub(crate) some_avg10: f64,
}

impl MemorySample {
    pub(crate) fn check(&self, reserve_bytes: u64) -> io::Result<()> {
        if self.available_bytes < reserve_bytes {
            return Err(guard_error(format!(
                "only {} bytes are available, below the {} byte reserve; close other workloads and retry",
                self.available_bytes, reserve_bytes
            )));
        }
        if !self.full_avg10.is_finite() || !self.some_avg10.is_finite() {
            return Err(guard_error(
                "memory pressure telemetry is invalid; close other workloads and retry",
            ));
        }
        if self.full_avg10 >= 1.0 {
            return Err(guard_error(format!(
                "full memory pressure is {:.2}% (limit 1.00%); close other workloads and retry",
                self.full_avg10
            )));
        }
        if self.some_avg10 >= 10.0 {
            return Err(guard_error(format!(
                "some memory pressure is {:.2}% (limit 10.00%); close other workloads and retry",
                self.some_avg10
            )));
        }
        Ok(())
    }
}

fn guard_error(message: impl Into<String>) -> io::Error {
    io::Error::other(format!("mutation resource guard: {}", message.into()))
}

#[cfg(target_os = "linux")]
mod cgroup;
#[cfg(target_os = "linux")]
mod paths;
#[cfg(target_os = "linux")]
mod procfs;

#[cfg(target_os = "linux")]
use std::path::Path;

#[cfg(target_os = "linux")]
const PROC_ROOT: &str = "/proc";

#[cfg(target_os = "linux")]
pub(crate) fn sample() -> io::Result<Option<MemorySample>> {
    sample_from_paths(Path::new(PROC_ROOT)).map(Some)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn sample() -> io::Result<Option<MemorySample>> {
    Ok(None)
}

#[cfg(target_os = "linux")]
fn sample_from_paths(proc_root: &Path) -> io::Result<MemorySample> {
    let (host_total, host_available) =
        procfs::parse_meminfo(&procfs::read_required(&proc_root.join("meminfo"))?)?;
    let host_pressure = procfs::read_pressure(&proc_root.join("pressure/memory"))?;
    let cgroup_data = procfs::read_required(&proc_root.join("self/cgroup"))?;
    let Some(cgroup_path) = paths::parse_cgroup_path(&cgroup_data)? else {
        return Ok(MemorySample {
            total_bytes: host_total,
            available_bytes: host_available,
            full_avg10: host_pressure.full_avg10,
            some_avg10: host_pressure.some_avg10,
        });
    };

    let mountinfo = procfs::read_required(&proc_root.join("self/mountinfo"))?;
    let mounts = paths::find_cgroup2_mounts(&mountinfo)?;
    let cgroup = cgroup::sample(&mounts, &cgroup_path)?;
    Ok(MemorySample {
        total_bytes: host_total.min(cgroup.total_limit.unwrap_or(host_total)),
        available_bytes: host_available.min(cgroup.available_headroom.unwrap_or(host_available)),
        full_avg10: host_pressure.full_avg10.max(cgroup.pressure.full_avg10),
        some_avg10: host_pressure.some_avg10.max(cgroup.pressure.some_avg10),
    })
}

#[cfg(target_os = "linux")]
fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("mutation resource guard: {}", message.into()),
    )
}

#[cfg(target_os = "linux")]
fn parse_decimal(value: &str, field: &str) -> io::Result<u64> {
    let valid = !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    if !valid {
        return Err(invalid_data(format!("invalid numeric value for {field}")));
    }
    value
        .parse::<u64>()
        .map_err(|_| invalid_data(format!("numeric value for {field} overflows")))
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "linux"))]
#[path = "memory/fixture_tests.rs"]
pub(super) mod fixture_tests;
