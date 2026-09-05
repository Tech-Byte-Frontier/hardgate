use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "shim.rs"]
mod shim;

const MAX_REPORT_BYTES: u64 = 16 * 1024;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub(super) struct CommandEvidence {
    directory: PathBuf,
    shim: PathBuf,
    report: PathBuf,
    identity: String,
    memory_bytes: u64,
    high_bytes: u64,
}

impl CommandEvidence {
    pub(super) fn create(memory_bytes: u64) -> io::Result<Self> {
        let high_bytes = crate::resources::MutationBudget::high_memory_bytes(memory_bytes);
        if high_bytes == 0 {
            return Err(resource_error("mutation memory.high limit is zero"));
        }
        let (directory, identity) = create_directory()?;
        let shim = directory.join("shim.sh");
        let report = directory.join("report");
        if let Err(error) = write_shim(&shim) {
            let _ = fs::remove_dir_all(&directory);
            return Err(error);
        }
        Ok(Self {
            directory,
            shim,
            report,
            identity,
            memory_bytes,
            high_bytes,
        })
    }

    pub(super) fn shim_path(&self) -> &Path {
        &self.shim
    }

    pub(super) fn report_path(&self) -> &Path {
        &self.report
    }

    pub(super) fn identity(&self) -> &str {
        &self.identity
    }

    pub(super) fn allow_start(&self) -> io::Result<()> {
        let marker = self.report.with_extension("start");
        let pending = self.report.with_extension("start-pending");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&pending)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    error
                } else {
                    resource_io("create pending mutation start marker", error)
                }
            })?;
        fs::set_permissions(&pending, fs::Permissions::from_mode(0o600))
            .map_err(|error| resource_io("protect pending mutation start marker", error))?;
        file.write_all(format!("{}\n", self.identity).as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| resource_io("write pending mutation start marker", error))?;
        drop(file);

        let link = fs::hard_link(&pending, &marker).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                error
            } else {
                resource_io("publish mutation start marker", error)
            }
        });
        let cleanup = fs::remove_file(&pending);
        match link {
            Ok(()) => {
                cleanup.map_err(|error| resource_io("remove pending mutation start marker", error))
            }
            Err(error) => {
                let _ = cleanup;
                Err(error)
            }
        }
    }

    pub(super) fn verify(&self, status: &ExitStatus) -> io::Result<()> {
        let manager_status = status
            .code()
            .ok_or_else(|| resource_error("mutation shim terminated by signal"))?;
        let report = read_report(&self.report)?;
        if report.identity != self.identity {
            return Err(resource_error("mutation evidence identity does not match"));
        }
        if report.limit != self.memory_bytes || report.high != self.high_bytes {
            return Err(resource_error(
                "mutation evidence memory limits do not match systemd",
            ));
        }
        if report.status != manager_status as u64 {
            return Err(resource_error(
                "mutation evidence exit status does not match systemd",
            ));
        }
        verify_events(&report.events)?;
        verify_pids_max_events(report.pids_max_events)?;
        if report.peak >= self.high_bytes {
            return Err(resource_error(
                "mutation process tree reached its memory.high limit",
            ));
        }
        Ok(())
    }
}

impl Drop for CommandEvidence {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

struct Report {
    identity: String,
    limit: u64,
    high: u64,
    status: u64,
    peak: u64,
    events: BTreeMap<String, u64>,
    pids_max_events: u64,
}

pub(super) fn check_live(cgroup: &Path, high_bytes: u64) -> io::Result<()> {
    let peak = read_counter_file(&cgroup.join("memory.peak"))?;
    if peak >= high_bytes {
        return Err(resource_error(
            "mutation process tree reached its memory.high limit",
        ));
    }
    let events = parse_events(&read_kernel_file(&cgroup.join("memory.events"))?)?;
    verify_events(&events)?;
    let pids_max_events = parse_pids_events(&read_kernel_file(&cgroup.join("pids.events"))?)?;
    verify_pids_max_events(pids_max_events)
}

fn create_directory() -> io::Result<(PathBuf, String)> {
    let root = std::env::temp_dir();
    for _ in 0..128 {
        if let Some(created) = create_directory_attempt(&root)? {
            return Ok(created);
        }
    }
    Err(resource_error(
        "could not allocate a unique mutation evidence directory",
    ))
}

fn create_directory_attempt(root: &Path) -> io::Result<Option<(PathBuf, String)>> {
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let identity = format!("{}-{counter}", std::process::id());
    let directory = root.join(format!("hardgate-resource-{identity}"));
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    match builder.create(&directory) {
        Ok(()) => {
            protect_directory(&directory)?;
            Ok(Some((directory, identity)))
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
        Err(error) => Err(resource_io("create mutation evidence directory", error)),
    }
}

fn protect_directory(directory: &Path) -> io::Result<()> {
    if let Err(error) = fs::set_permissions(directory, fs::Permissions::from_mode(0o700)) {
        let error = resource_io("protect mutation evidence directory", error);
        let _ = fs::remove_dir_all(directory);
        return Err(error);
    }
    if let Err(error) = verify_directory(directory) {
        let _ = fs::remove_dir_all(directory);
        return Err(error);
    }
    Ok(())
}

fn verify_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| resource_io("inspect mutation evidence directory", error))?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.mode() & 0o7777 != 0o700
    {
        return Err(resource_error(
            "mutation evidence directory is not a private owned directory",
        ));
    }
    Ok(())
}

fn write_shim(path: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| resource_io("create mutation evidence shim", error))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| resource_io("protect mutation evidence shim", error))?;
    file.write_all(shim::SOURCE)
        .and_then(|()| file.sync_all())
        .map_err(|error| resource_io("write mutation evidence shim", error))?;
    Ok(())
}

fn read_report(path: &Path) -> io::Result<Report> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| resource_error("mutation evidence report is missing"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() || metadata.nlink() != 1
    {
        return Err(resource_error(
            "mutation evidence report is not a regular single-link file",
        ));
    }
    let bytes = read_bounded(path)?;
    let content = String::from_utf8(bytes)
        .map_err(|_| resource_error("mutation evidence report is not UTF-8"))?;
    parse_report(&content)
}

fn parse_report(content: &str) -> io::Result<Report> {
    let mut fields = BTreeMap::new();
    for line in content.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| resource_error("mutation evidence report has an invalid line"))?;
        if key.is_empty() || fields.insert(key, value).is_some() {
            return Err(resource_error(
                "mutation evidence report has duplicate fields",
            ));
        }
    }
    let identity = fields
        .remove("id")
        .ok_or_else(|| resource_error("mutation evidence report is missing id"))?;
    let limit = parse_decimal(
        fields
            .remove("limit")
            .ok_or_else(|| resource_error("mutation evidence report is missing limit"))?,
        "limit",
    )?;
    let high = parse_decimal(
        fields
            .remove("high")
            .ok_or_else(|| resource_error("mutation evidence report is missing high"))?,
        "high",
    )?;
    let status = parse_decimal(
        fields
            .remove("status")
            .ok_or_else(|| resource_error("mutation evidence report is missing status"))?,
        "status",
    )?;
    let peak = parse_decimal(
        fields
            .remove("peak")
            .ok_or_else(|| resource_error("mutation evidence report is missing peak"))?,
        "peak",
    )?;
    let pids_max_events = parse_decimal(
        fields
            .remove("pids_max_events")
            .ok_or_else(|| resource_error("mutation evidence report is missing pids_max_events"))?,
        "pids.events max",
    )?;
    let mut events = BTreeMap::new();
    for (key, value) in fields {
        let Some(event) = key.strip_prefix("events.") else {
            return Err(resource_error(
                "mutation evidence report has an unknown field",
            ));
        };
        if event.is_empty() || !valid_event_name(event) {
            return Err(resource_error(
                "mutation evidence report has an invalid event",
            ));
        }
        let value = parse_decimal(value, "memory.events")?;
        if events.insert(event.to_string(), value).is_some() {
            return Err(resource_error("mutation evidence report repeats an event"));
        }
    }
    Ok(Report {
        identity: identity.to_string(),
        limit,
        high,
        status,
        peak,
        events,
        pids_max_events,
    })
}

fn verify_events(events: &BTreeMap<String, u64>) -> io::Result<()> {
    verify_required_events(events)?;
    for key in ["high", "max", "oom", "oom_kill", "oom_group_kill"] {
        if events.get(key).is_some_and(|value| *value != 0) {
            return Err(resource_error(&format!(
                "memory.events reports a nonzero {key} counter"
            )));
        }
    }
    Ok(())
}

fn verify_pids_max_events(value: u64) -> io::Result<()> {
    if value != 0 {
        return Err(resource_error("pids.events reports a nonzero max counter"));
    }
    Ok(())
}

fn parse_events(content: &str) -> io::Result<BTreeMap<String, u64>> {
    let mut events = BTreeMap::new();
    for line in content.lines() {
        let (key, value) = parse_counter_fields(line, "memory.events")?;
        if !valid_event_name(key) {
            return Err(resource_error("memory.events has an invalid line"));
        }
        let value = parse_decimal(value, "memory.events")?;
        if events.insert(key.to_string(), value).is_some() {
            return Err(resource_error("memory.events repeats an event"));
        }
    }
    verify_required_events(&events)?;
    Ok(events)
}

fn parse_pids_events(content: &str) -> io::Result<u64> {
    let mut lines = content.lines();
    let line = lines
        .next()
        .ok_or_else(|| resource_error("pids.events is missing max"))?;
    if lines.next().is_some() {
        return Err(resource_error("pids.events has more than one line"));
    }
    let (key, value) = parse_counter_fields(line, "pids.events")?;
    if key != "max" {
        return Err(resource_error("pids.events has an invalid line"));
    }
    let value = parse_decimal(value, "pids.events max")?;
    verify_pids_max_events(value)?;
    Ok(value)
}

fn parse_counter_fields<'a>(line: &'a str, source: &str) -> io::Result<(&'a str, &'a str)> {
    let mut fields = line.split_whitespace();
    let key = fields
        .next()
        .ok_or_else(|| resource_error(&format!("{source} has an invalid line")))?;
    let value = fields
        .next()
        .ok_or_else(|| resource_error(&format!("{source} has a missing counter")))?;
    if fields.next().is_some() {
        return Err(resource_error(&format!("{source} has an invalid line")));
    }
    Ok((key, value))
}

fn verify_required_events(events: &BTreeMap<String, u64>) -> io::Result<()> {
    for required in ["high", "max", "oom", "oom_kill"] {
        if !events.contains_key(required) {
            return Err(resource_error(&format!(
                "memory.events is missing {required}"
            )));
        }
    }
    Ok(())
}

fn read_counter_file(path: &Path) -> io::Result<u64> {
    parse_decimal(&read_kernel_file(path)?, "cgroup counter")
}

fn read_kernel_file(path: &Path) -> io::Result<String> {
    let bytes = read_bounded(path)?;
    let content =
        String::from_utf8(bytes).map_err(|_| resource_error("cgroup evidence is not UTF-8"))?;
    Ok(content.trim_end_matches('\n').to_string())
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            error
        } else {
            resource_io("read mutation evidence", error)
        }
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| resource_io("read mutation evidence", error))?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(resource_error("mutation evidence exceeds 16 KiB"));
    }
    Ok(bytes)
}

fn parse_decimal(value: &str, field: &str) -> io::Result<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(resource_error(&format!("invalid numeric {field} evidence")));
    }
    value
        .parse::<u64>()
        .map_err(|_| resource_error(&format!("numeric {field} evidence overflows")))
}

fn valid_event_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn resource_error(message: &str) -> io::Error {
    io::Error::other(format!("mutation resource guard: {message}"))
}

fn resource_io(operation: &str, error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::NotFound {
        error
    } else {
        io::Error::other(format!("mutation resource guard: {operation}: {error}"))
    }
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "evidence_failure_tests.rs"]
mod failure_tests;
