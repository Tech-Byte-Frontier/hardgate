use super::evidence::CommandEvidence;
use super::{DESCRIPTION, UNIT, control_output, resource_error};
use crate::resources::MutationBudget;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

// Match execvp's unset-PATH default on the supported GNU target.
const DEFAULT_PATH: &str = "/bin:/usr/bin";

pub(super) fn available_tools() -> io::Result<Option<(PathBuf, PathBuf)>> {
    // Scope commands execute in the caller's environment and namespaces;
    // the user manager applies resource limits to that existing process.
    let uid = rustix::process::getuid().as_raw();
    let runtime = PathBuf::from(format!("/run/user/{uid}"));
    available_tools_at(&runtime, uid, None)
}

fn available_tools_at(
    runtime: &Path,
    uid: u32,
    path: Option<&OsStr>,
) -> io::Result<Option<(PathBuf, PathBuf)>> {
    if !fs::symlink_metadata(runtime)
        .is_ok_and(|meta| meta.is_dir() && meta.uid() == uid && meta.mode() & 0o077 == 0)
    {
        return Ok(None);
    }
    let socket = runtime.join("systemd/private");
    if !fs::symlink_metadata(&socket)
        .is_ok_and(|meta| meta.file_type().is_socket() && meta.uid() == uid)
    {
        return Ok(None);
    }
    let Some(launcher) = find_program(OsStr::new("systemd-run"), path, None) else {
        return Ok(None);
    };
    let Some(controller) = find_program(OsStr::new("systemctl"), path, None) else {
        return Ok(None);
    };
    // v254 introduced literal argument handling. Older managers are refused; silently expanding a test command's '$' is unsafe.
    let version = control_output(&[launcher.to_string_lossy().into_owned(), "--version".into()])?;
    if systemd_version(&version).is_none_or(|version| version < 254) {
        return Ok(None);
    }
    Ok(Some((launcher, controller)))
}

pub(super) fn systemd_version(output: &str) -> Option<u32> {
    let mut words = output.lines().next()?.split_whitespace();
    (words.next()? == "systemd")
        .then(|| words.next()?.parse().ok())
        .flatten()
}

pub(super) struct LaunchSettings<'a> {
    pub(super) budget: MutationBudget,
    pub(super) timeout: Duration,
    pub(super) evidence: &'a CommandEvidence,
}

pub(super) fn wrap_command(
    original: &Command,
    launcher: &Path,
    settings: LaunchSettings<'_>,
) -> io::Result<Command> {
    let LaunchSettings {
        budget,
        timeout,
        evidence,
    } = settings;
    let mut command = Command::new(launcher);
    command.args([
        "--user",
        "--quiet",
        "--scope",
        "--collect",
        "--expand-environment=no",
    ]);
    command.arg(format!("--unit={UNIT}"));
    command.arg(format!(
        "--description={DESCRIPTION} {}",
        evidence.identity()
    ));
    for property in [
        format!("MemoryHigh={}", budget.high_bytes()),
        format!("MemoryMax={}", budget.memory_bytes),
        "MemorySwapMax=0".into(),
        "MemoryAccounting=yes".into(),
        "OOMPolicy=kill".into(),
        "KillMode=control-group".into(),
        "TimeoutStopSec=1s".into(),
        format!(
            "TasksMax={}",
            crate::resources::runtime::profile::task_limit(budget.jobs)
        ),
        format!("CPUQuota={}%", budget.jobs * 100),
        format!("RuntimeMaxSec={}s", timeout.as_secs().saturating_add(5)),
    ] {
        command.arg(format!("--property={property}"));
    }
    if let Some(directory) = original.get_current_dir() {
        command.current_dir(directory);
    }
    inherit_environment(original, &mut command);
    command
        .env(
            "XDG_RUNTIME_DIR",
            format!("/run/user/{}", rustix::process::getuid().as_raw()),
        )
        .env_remove("DBUS_SESSION_BUS_ADDRESS");
    let program = resolve_original_program(original)?;
    command
        .args(["--", "/bin/sh"])
        .arg(evidence.shim_path())
        .arg(evidence.report_path())
        .arg(budget.memory_bytes.to_string())
        .arg(budget.high_bytes().to_string())
        .arg(evidence.identity())
        .arg(crate::resources::runtime::profile::task_limit(budget.jobs).to_string())
        .arg(program)
        .args(original.get_args());
    Ok(command)
}

fn inherit_environment(original: &Command, wrapped: &mut Command) {
    for (key, value) in original.get_envs() {
        match value {
            Some(value) => {
                wrapped.env(key, value);
            }
            None => {
                wrapped.env_remove(key);
            }
        }
    }
}

fn resolve_original_program(command: &Command) -> io::Result<PathBuf> {
    let program = Path::new(command.get_program());
    if program.components().count() > 1 {
        return Ok(if program.is_absolute() {
            program.to_path_buf()
        } else {
            std::env::current_dir()?
                .join(command.get_current_dir().unwrap_or(Path::new(".")))
                .join(program)
        });
    }
    let path = command
        .get_envs()
        .find(|(name, _)| *name == "PATH")
        .map(|(_, value)| value.unwrap_or(OsStr::new(DEFAULT_PATH)));
    find_program(command.get_program(), path, command.get_current_dir()).ok_or_else(|| {
        resource_error("Failed to execute mutation command: executable was not found in PATH")
    })
}

fn find_program(
    program: &OsStr,
    path: Option<&OsStr>,
    directory: Option<&Path>,
) -> Option<PathBuf> {
    let inherited = std::env::var_os("PATH");
    let path = path
        .or(inherited.as_deref())
        .unwrap_or(OsStr::new(DEFAULT_PATH));
    let directory = std::env::current_dir()
        .ok()?
        .join(directory.unwrap_or(Path::new(".")));
    std::env::split_paths(path)
        .map(|entry| directory.join(entry).join(program))
        .find(|entry| executable_file(entry))
}

fn executable_file(path: &Path) -> bool {
    path.is_file()
        && rustix::fs::accessat(
            rustix::fs::CWD,
            path,
            rustix::fs::Access::EXEC_OK,
            rustix::fs::AtFlags::EACCESS,
        )
        .is_ok()
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
