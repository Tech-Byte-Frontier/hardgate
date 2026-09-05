use super::paths::{self, CgroupMount};
use super::procfs::{self, Pressure};
use super::{invalid_data, parse_decimal};
use std::ffi::OsString;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CgroupTelemetry {
    pub(super) total_limit: Option<u64>,
    pub(super) available_headroom: Option<u64>,
    pub(super) pressure: Pressure,
}

#[derive(Clone, Copy, Debug)]
struct Level {
    maximum: Option<u64>,
    high: Option<u64>,
    current: Option<u64>,
    pressure: Option<Pressure>,
}

pub(super) fn sample(
    mounts: &[CgroupMount],
    cgroup_path: &[OsString],
) -> io::Result<CgroupTelemetry> {
    let mut state = SampleState::default();
    sample_matching_mounts(mounts, cgroup_path, true, &mut state)?;
    sample_matching_mounts(mounts, cgroup_path, false, &mut state)?;
    state.finish()
}

#[derive(Default)]
struct SampleState {
    last_missing: Option<io::Error>,
    combined: Option<CgroupTelemetry>,
}

impl SampleState {
    fn record(&mut self, result: io::Result<CgroupTelemetry>) -> io::Result<()> {
        match result {
            Ok(telemetry) => merge(&mut self.combined, telemetry),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.last_missing = Some(error);
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }

    fn finish(self) -> io::Result<CgroupTelemetry> {
        let Self {
            combined,
            last_missing,
        } = self;
        combined.ok_or_else(|| {
            last_missing.unwrap_or_else(|| invalid_data("no reachable cgroup2 mount"))
        })
    }
}

fn sample_matching_mounts(
    mounts: &[CgroupMount],
    cgroup_path: &[OsString],
    matching_root: bool,
    state: &mut SampleState,
) -> io::Result<()> {
    for mount in mounts
        .iter()
        .filter(|mount| paths::mount_root_matches(mount, cgroup_path) == matching_root)
    {
        let directories = paths::cgroup_directories(mount, cgroup_path)?;
        state.record(sample_mount(&directories))?;
    }
    Ok(())
}

fn sample_mount(directories: &[std::path::PathBuf]) -> io::Result<CgroupTelemetry> {
    let mut telemetry = CgroupTelemetry::default();
    for (index, directory) in directories.iter().enumerate() {
        let level = read_level(directory, index + 1 == directories.len())?;
        if let (Some(maximum), Some(current)) = (level.maximum, level.current) {
            telemetry.total_limit = Some(minimum(telemetry.total_limit, maximum));
            let headroom = maximum.saturating_sub(current);
            telemetry.available_headroom = Some(minimum(telemetry.available_headroom, headroom));
        }
        if let (Some(high), Some(current)) = (level.high, level.current) {
            let headroom = high.saturating_sub(current);
            telemetry.available_headroom = Some(minimum(telemetry.available_headroom, headroom));
        }
        if let Some(pressure) = level.pressure {
            telemetry.pressure.full_avg10 = telemetry.pressure.full_avg10.max(pressure.full_avg10);
            telemetry.pressure.some_avg10 = telemetry.pressure.some_avg10.max(pressure.some_avg10);
        }
    }
    Ok(telemetry)
}

fn read_level(directory: &Path, root: bool) -> io::Result<Level> {
    if !procfs::directory_exists(directory)? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "mutation resource guard: cgroup directory is unreachable: {}",
                directory.display()
            ),
        ));
    }
    let pressure = procfs::read_optional(&directory.join("memory.pressure"))?
        .map(|content| procfs::parse_pressure(&content))
        .transpose()?;
    let maximum = procfs::read_optional(&directory.join("memory.max"))?;
    let high = procfs::read_optional(&directory.join("memory.high"))?;
    let current = procfs::read_optional(&directory.join("memory.current"))?;
    if root {
        match (maximum.as_ref(), high.as_ref(), current.as_ref()) {
            (None, None, None) => {
                return Ok(Level {
                    maximum: None,
                    high: None,
                    current: None,
                    pressure,
                });
            }
            (Some(_), Some(_), Some(_)) => {}
            _ => {
                return Err(invalid_data(
                    "partial memory controller telemetry in cgroup root",
                ));
            }
        }
    }
    let maximum = parse_limit(
        maximum.ok_or_else(|| missing_file(directory, "memory.max"))?,
        "memory.max",
    )?;
    let high = parse_limit(
        high.ok_or_else(|| missing_file(directory, "memory.high"))?,
        "memory.high",
    )?;
    let current = parse_counter(
        current.ok_or_else(|| missing_file(directory, "memory.current"))?,
        "memory.current",
    )?;
    Ok(Level {
        maximum,
        high,
        current: Some(current),
        pressure,
    })
}

fn parse_limit(value: String, field: &str) -> io::Result<Option<u64>> {
    let value = value.trim();
    if value == "max" {
        return Ok(None);
    }
    parse_counter(value.to_string(), field).map(Some)
}

fn parse_counter(value: String, field: &str) -> io::Result<u64> {
    let value = value.trim();
    if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(invalid_data(format!("invalid counter for {field}")));
    }
    parse_decimal(value, field)
}

fn minimum(current: Option<u64>, next: u64) -> u64 {
    current.map_or(next, |current| current.min(next))
}

fn missing_file(directory: &Path, file: &str) -> io::Error {
    invalid_data(format!(
        "missing {file} in discovered cgroup {}",
        directory.display()
    ))
}

fn merge(combined: &mut Option<CgroupTelemetry>, next: CgroupTelemetry) {
    let Some(current) = combined.as_mut() else {
        *combined = Some(next);
        return;
    };
    current.total_limit = merge_min(current.total_limit, next.total_limit);
    current.available_headroom = merge_min(current.available_headroom, next.available_headroom);
    current.pressure.full_avg10 = current.pressure.full_avg10.max(next.pressure.full_avg10);
    current.pressure.some_avg10 = current.pressure.some_avg10.max(next.pressure.some_avg10);
}

fn merge_min(current: Option<u64>, next: Option<u64>) -> Option<u64> {
    match (current, next) {
        (Some(current), Some(next)) => Some(current.min(next)),
        (Some(current), None) => Some(current),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
}
