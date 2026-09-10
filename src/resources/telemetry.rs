//! Diagnostic samples never substitute for the authoritative admission guard.
use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub(crate) struct MemoryTelemetry {
    host_available_bytes: Option<u64>,
    cgroup_limit_bytes: Option<u64>,
    cgroup_available_bytes: Option<u64>,
    cgroup_current_bytes: Option<u64>,
    cgroup_peak_bytes: Option<u64>,
    pub rss_bytes: Option<u64>,
    limiting_domain: &'static str,
}

pub(crate) fn memory() -> MemoryTelemetry {
    #[cfg(target_os = "linux")]
    return linux().unwrap_or_default();
    #[cfg(not(target_os = "linux"))]
    MemoryTelemetry::default()
}

pub(crate) fn pressure_error(error: std::io::Error) -> std::io::Error {
    let telemetry = memory();
    std::io::Error::new(
        error.kind(),
        format!(
            "{error}; memory telemetry: {}",
            serde_json::to_string(&telemetry).unwrap_or_default()
        ),
    )
}

#[cfg(target_os = "linux")]
fn linux() -> std::io::Result<MemoryTelemetry> {
    let host = super::memory::host_available_bytes()?;
    let sample =
        super::memory::sample()?.ok_or_else(|| std::io::Error::other("missing memory sample"))?;
    let directories = super::memory::runtime_directories()?;
    let Some(directory) = directories.first() else {
        return Ok(MemoryTelemetry {
            host_available_bytes: Some(host),
            limiting_domain: "host",
            ..Default::default()
        });
    };
    Ok(MemoryTelemetry {
        host_available_bytes: Some(host),
        cgroup_limit_bytes: Some(sample.total_bytes),
        cgroup_available_bytes: Some(sample.available_bytes),
        cgroup_current_bytes: counter(directory, "memory.current"),
        cgroup_peak_bytes: counter(directory, "memory.peak"),
        rss_bytes: workload_rss(directory),
        limiting_domain: if sample.available_bytes < host {
            "cgroup"
        } else {
            "host"
        },
    })
}

#[cfg(target_os = "linux")]
fn counter(directory: &std::path::Path, name: &str) -> Option<u64> {
    std::fs::read_to_string(directory.join(name))
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
fn workload_rss(directory: &std::path::Path) -> Option<u64> {
    let pids = std::fs::read_to_string(directory.join("cgroup.procs")).ok()?;
    let mut total = 0u64;
    for pid in pids.lines() {
        let pid: u32 = pid.parse().ok()?;
        let status = match std::fs::read_to_string(format!("/proc/{pid}/status")) {
            Ok(status) => status,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        let rss = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))?;
        let kib: u64 = rss.split_whitespace().next()?.parse().ok()?;
        total = total.checked_add(kib.checked_mul(1024)?)?;
    }
    Some(total)
}

#[cfg(all(test, target_os = "linux"))]
#[path = "telemetry_tests.rs"]
mod tests;
