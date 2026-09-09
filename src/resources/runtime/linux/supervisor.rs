use super::CHILD_MARKER;
use crate::resources::runtime::profile;
use crate::resources::runtime::{constrain_command, error};
use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "supervisor-readiness.rs"]
mod readiness;
pub(super) fn acknowledge() -> io::Result<()> {
    readiness::acknowledge()
}

pub(super) fn run(memory: u64) -> io::Result<u8> {
    let runtime = runtime_directory()?;
    let _lease = crate::resources::lease::acquire_workload()?;
    _lease.inherit_workload()?;
    let identity = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(error)?
            .as_nanos()
    );
    let ready = readiness::Readiness::create(&identity)?;
    let command = launch(&runtime, &ready, memory)?;
    supervise(command, &ready)
}

fn supervise(mut command: Command, ready: &readiness::Readiness) -> io::Result<u8> {
    let mut child = command.spawn().map_err(|cause| {
        error(format!(
            "cannot launch the systemd user scope ({cause}); no workload was started"
        ))
    })?;
    let result = wait(&mut child, ready);
    let cleanup = if ready.observed() {
        stop_scope(ready)
    } else {
        Ok(())
    };
    let result = match result {
        Ok(0 | 1) if !ready.observed() => Err(error(
            "the systemd user manager did not establish the owned workload scope; no evaluation was acknowledged",
        )),
        other => other,
    };
    match (result, cleanup) {
        (Ok(code), Ok(())) => Ok(code),
        (Err(cause), _) | (_, Err(cause)) => Err(cause),
    }
}

fn runtime_directory() -> io::Result<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let directory = PathBuf::from(format!("/run/user/{uid}"));
    validate_runtime(&directory, uid)?;
    Ok(directory)
}

fn validate_runtime(directory: &Path, uid: u32) -> io::Result<()> {
    let metadata = fs::symlink_metadata(directory).map_err(|_| {
        error("an accessible systemd user manager is required; no workload was started")
    })?;
    let socket = fs::symlink_metadata(directory.join("systemd/private"))?;
    if !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.mode() & 0o077 != 0
        || !socket.file_type().is_socket()
        || socket.uid() != uid
    {
        return Err(error(
            "the user-manager runtime directory or socket is not owner-validated",
        ));
    }
    Ok(())
}

fn launch(runtime: &Path, ready: &readiness::Readiness, memory: u64) -> io::Result<Command> {
    let mut command = Command::new("systemd-run");
    command
        .args([
            "--user",
            "--quiet",
            "--scope",
            "--collect",
            "--expand-environment=no",
        ])
        .arg(format!("--unit={}", ready.1))
        .arg(format!("--description={}", description(&ready.0)))
        .env("XDG_RUNTIME_DIR", runtime)
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env(CHILD_MARKER, &ready.0);
    let jobs = profile::jobs();
    let quota = jobs * 100;
    for property in [
        format!("CPUQuota={quota}%"),
        "CPUWeight=25".into(),
        format!("MemoryMax={memory}"),
        format!(
            "MemoryHigh={}",
            crate::resources::MutationBudget::high_memory_bytes(memory)
        ),
        "MemorySwapMax=0".into(),
        format!("TasksMax={}", profile::task_limit(jobs)),
        "OOMPolicy=kill".into(),
        "KillMode=control-group".into(),
        "TimeoutStopSec=1s".into(),
        "RuntimeMaxSec=1800s".into(),
    ] {
        command.arg(format!("--property={property}"));
    }
    constrain_command(&mut command);
    command
        .arg("--")
        .arg(std::env::current_exe()?)
        .args(std::env::args_os().skip(1));
    Ok(command)
}

fn wait(child: &mut Child, ready: &readiness::Readiness) -> io::Result<u8> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return exit_code(status);
        }
        if crate::cancellation::signal().is_some() || start.elapsed() > Duration::from_secs(1810) {
            return cancel(child, ready);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn exit_code(status: std::process::ExitStatus) -> io::Result<u8> {
    match status.code() {
        Some(code @ (0..=2 | 130 | 143)) => Ok(code as u8),
        _ => Err(error(format!(
            "workload terminated ({status}); evidence is incomplete"
        ))),
    }
}

fn cancel(child: &mut Child, ready: &readiness::Readiness) -> io::Result<u8> {
    if ready.observed() {
        stop_scope(ready)?;
    }
    let _ = child.kill();
    child.wait()?;
    Err(error(
        "workload cancelled or exceeded its 30-minute runtime limit",
    ))
}

fn description(ready: &Path) -> String {
    format!(
        "Hardgate workload {}",
        ready.parent().unwrap_or(ready).display()
    )
}

fn manager(args: &[&str]) -> io::Result<String> {
    use crate::engines::process::{ProcessOutcome, run_control_command};
    let mut tokens = vec!["systemctl".to_owned(), "--user".to_owned()];
    tokens.extend(args.iter().map(|arg| (*arg).to_owned()));
    match run_control_command(&tokens) {
        ProcessOutcome::Completed { status, output } if status.success() => Ok(output),
        _ => Err(error(
            "could not verify or clean up the owned workload scope",
        )),
    }
}

fn owned_active(ready: &readiness::Readiness) -> io::Result<bool> {
    let output = manager(&[
        "show",
        &ready.1,
        "--property=LoadState,ActiveState,Description",
    ])?;
    scope_active(&output, &description(&ready.0))
}

fn scope_active(output: &str, expected: &str) -> io::Result<bool> {
    let fields = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<std::collections::BTreeMap<_, _>>();
    if fields.get("LoadState") == Some(&"not-found") {
        return Ok(false);
    }
    if fields.get("Description").copied() != Some(expected) {
        return Err(error(
            "reserved workload scope has a different owner identity",
        ));
    }
    match fields.get("ActiveState").copied() {
        Some("inactive" | "failed") => Ok(false),
        Some("active" | "activating" | "deactivating") => Ok(true),
        _ => Err(error("missing workload scope state")),
    }
}

fn stop_scope(ready: &readiness::Readiness) -> io::Result<()> {
    if !owned_active(ready)? {
        return Ok(());
    }
    manager(&["stop", &ready.1])?;
    if owned_active(ready)? {
        return Err(error("owned workload scope remains active after cleanup"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;
