use super::{invalid_data, parse_decimal, procfs};
use std::io;
use std::path::Path;

/// Clean disk cache can be reclaimed without swap. Keep anonymous memory,
/// tmpfs, dirty pages, writeback, and kernel memory in the working estimate.
/// Hard cgroup limits and PSI checks remain independent of this estimate.
pub(super) fn working_bytes(directory: &Path, current: u64) -> io::Result<u64> {
    let Some(stat) = procfs::read_optional(&directory.join("memory.stat"))? else {
        return Ok(current);
    };
    let mut counters = std::collections::BTreeMap::new();
    for line in stat.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 2 {
            return Err(invalid_data("invalid memory.stat row"));
        }
        let value = parse_decimal(fields[1], fields[0])?;
        if counters.insert(fields[0], value).is_some() {
            return Err(invalid_data("duplicate memory.stat counter"));
        }
    }
    let required = ["file", "shmem", "file_dirty", "file_writeback"];
    if required.iter().any(|name| !counters.contains_key(name)) {
        return Err(invalid_data("missing file-cache telemetry"));
    }
    let clean = counters["file"]
        .saturating_sub(counters["shmem"])
        .saturating_sub(counters["file_dirty"])
        .saturating_sub(counters["file_writeback"])
        .saturating_sub(counters.get("unevictable").copied().unwrap_or(0));
    // Telemetry is not atomic: never subtract more than current usage.
    Ok(current.saturating_sub(clean.min(current)))
}

#[cfg(test)]
#[path = "reclaim_tests.rs"]
mod tests;
