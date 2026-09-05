use crate::engines::process::{ProcessOutcome, run_control_command};
use crate::resources::MutationBudget;
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

#[path = "evidence.rs"]
mod evidence;
#[path = "launch.rs"]
mod launch;
#[path = "status.rs"]
mod status;
use evidence::CommandEvidence;
use launch::{LaunchSettings, available_tools, wrap_command};
use status::UnitStatus;

const UNIT: &str = "hardgate-native-mutation.scope";
const DESCRIPTION: &str = "Hardgate native mutation resource boundary";
const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) struct ManagedCommand {
    controller: PathBuf,
    high_bytes: u64,
    evidence: CommandEvidence,
    invocation: Option<String>,
    cgroup: Option<PathBuf>,
    cgroup_directory: Option<fs::File>,
    last_poll: Option<Instant>,
    owned: bool,
    started: Option<Instant>,
}

impl ManagedCommand {
    pub(crate) fn prepare(
        command: &mut Command,
        budget: MutationBudget,
        timeout: Duration,
    ) -> io::Result<Option<Self>> {
        let Some((launcher, controller)) = available_tools()? else {
            return Ok(None);
        };
        let existing = inspect(&controller)?;
        reject_previous(&existing)?;
        let evidence = CommandEvidence::create(budget.memory_bytes)?;
        let settings = LaunchSettings {
            budget,
            timeout,
            evidence: &evidence,
        };
        let wrapped = wrap_command(command, &launcher, settings)?;
        *command = wrapped;
        Ok(Some(Self {
            controller,
            high_bytes: budget.high_bytes(),
            evidence,
            invocation: None,
            cgroup: None,
            cgroup_directory: None,
            last_poll: None,
            owned: true,
            started: None,
        }))
    }

    pub(crate) fn poll(&mut self, exited: Option<ExitStatus>) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = exited {
            self.evidence.verify(&status)?;
            return Ok(Some(status));
        }
        if self
            .last_poll
            .is_some_and(|at| at.elapsed() < POLL_INTERVAL)
        {
            return Ok(None);
        }
        let status = inspect(&self.controller)?;
        self.last_poll = Some(Instant::now());
        self.observe(&status)?;
        self.start_when_pinned()?;
        self.check_current_cgroup()?;
        Ok(None)
    }

    fn start_when_pinned(&mut self) -> io::Result<()> {
        if self.started.is_none() && self.cgroup_directory.is_some() && self.invocation.is_some() {
            self.evidence.allow_start()?;
            self.started = Some(Instant::now());
        }
        Ok(())
    }

    pub(crate) fn timed_out(&self, launched: Instant, timeout: Duration) -> bool {
        match self.started {
            Some(started) => started.elapsed() >= timeout,
            None => launched.elapsed() >= timeout.saturating_add(Duration::from_secs(5)),
        }
    }

    fn check_current_cgroup(&self) -> io::Result<()> {
        let Some(cgroup) = &self.cgroup else {
            return Ok(());
        };
        match evidence::check_live(cgroup, self.high_bytes) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Scopes can be collected before the direct child is reaped.
                // Only an empty pinned group permits that terminal race;
                // the next child poll still requires the shim's final report.
                if self.cgroup_directory.as_ref().is_some_and(|directory| {
                    pinned_cgroup_populated(directory).is_ok_and(|populated| !populated)
                }) {
                    Ok(())
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    fn observe(&mut self, status: &UnitStatus) -> io::Result<()> {
        if !status.present() {
            return Ok(());
        }
        status.verify_identity(self.evidence.identity(), self.invocation.as_deref())?;
        if let Some(invocation) = status.invocation()? {
            self.invocation = Some(invocation.to_owned());
        }
        if let Some(cgroup) = status.cgroup()? {
            self.pin_cgroup(&cgroup)?;
            self.cgroup = Some(cgroup);
        }
        Ok(())
    }

    fn pin_cgroup(&mut self, cgroup: &Path) -> io::Result<()> {
        if self.cgroup_directory.is_some() {
            return Ok(());
        }
        match fs::File::open(cgroup) {
            Ok(directory) => self.cgroup_directory = Some(directory),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }

    pub(crate) fn stop(&mut self) -> io::Result<()> {
        if !self.owned {
            return Ok(());
        }
        match self.stop_owned() {
            Ok(()) => {
                self.owned = false;
                Ok(())
            }
            Err(error) => {
                if let Some(directory) = &self.cgroup_directory {
                    if let Err(cleanup) = stop_pinned_cgroup(directory) {
                        return Err(resource_error(&format!(
                            "{error}; cleanup failed: {cleanup}"
                        )));
                    }
                    self.owned = false;
                }
                Err(error)
            }
        }
    }

    fn stop_owned(&mut self) -> io::Result<()> {
        let status = inspect(&self.controller)?;
        if !status.present() {
            return self
                .cgroup_directory
                .as_ref()
                .map_or(Ok(()), stop_pinned_cgroup);
        }
        self.observe(&status)?;
        control(&self.controller, &["stop", UNIT])?;
        if let Some(directory) = &self.cgroup_directory {
            stop_pinned_cgroup(directory)?;
        }
        let stopped = inspect(&self.controller)?;
        if stopped.present() {
            self.observe(&stopped)?;
            if stopped.running() {
                return Err(resource_error("systemd did not stop the mutation scope"));
            }
            control(&self.controller, &["reset-failed", UNIT])?;
        }
        self.wait_until_collected()
    }

    fn wait_until_collected(&mut self) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = inspect(&self.controller)?;
            if !status.present() {
                return Ok(());
            }
            self.observe(&status)?;
            if Instant::now() >= deadline {
                return Err(resource_error(
                    "stopped mutation scope was not collected; retry after it disappears",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for ManagedCommand {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("hardgate: {error}");
        }
    }
}

fn stop_pinned_cgroup(directory: &fs::File) -> io::Result<()> {
    if !pinned_cgroup_populated(directory)? {
        return Ok(());
    }
    // Resolve relative to the original directory inode. Reuse of the unit
    // name must never make cleanup kill a later invocation's cgroup.
    let fd = rustix::fs::openat(
        directory,
        "cgroup.kill",
        rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    fs::File::from(fd).write_all(b"1")?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while pinned_cgroup_populated(directory)? {
        if Instant::now() >= deadline {
            return Err(resource_error(
                "owned mutation cgroup did not become empty after cleanup",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn pinned_cgroup_populated(directory: &fs::File) -> io::Result<bool> {
    let fd = rustix::fs::openat(
        directory,
        "cgroup.events",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    );
    let fd = match fd {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) if pinned_cgroup_removed(directory)? => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut content = String::new();
    fs::File::from(fd).take(1025).read_to_string(&mut content)?;
    if content.len() > 1024 {
        return Err(resource_error("oversized cgroup cleanup evidence"));
    }
    match content
        .lines()
        .find_map(|line| line.strip_prefix("populated "))
    {
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => Err(resource_error("missing or invalid cgroup cleanup evidence")),
    }
}

fn pinned_cgroup_removed(directory: &fs::File) -> io::Result<bool> {
    // kernfs keeps st_nlink at 2 after removal. The pinned descriptor's
    // procfs link records deletion without resolving a potentially reused
    // unit path; a populated cgroup cannot be removed by the kernel.
    let link = fs::read_link(format!("/proc/self/fd/{}", directory.as_raw_fd()))?;
    Ok(link
        .file_name()
        .is_some_and(|name| name == std::ffi::OsStr::new(&format!("{UNIT} (deleted)"))))
}

fn reject_previous(status: &UnitStatus) -> io::Result<()> {
    if status.present() {
        Err(resource_error(
            "the reserved mutation scope already exists; inspect hardgate-native-mutation.scope before retrying",
        ))
    } else {
        Ok(())
    }
}

fn inspect(controller: &Path) -> io::Result<UnitStatus> {
    let output = control(
        controller,
        &[
            "show",
            UNIT,
            "--property=LoadState,ActiveState,SubState,Result,Description,Transient,ControlGroup,InvocationID",
        ],
    )?;
    UnitStatus::parse(&output)
}

fn control(controller: &Path, args: &[&str]) -> io::Result<String> {
    let mut tokens = vec![controller.to_string_lossy().into_owned(), "--user".into()];
    tokens.extend(args.iter().map(|value| (*value).to_string()));
    control_output(&tokens)
}

fn control_output(tokens: &[String]) -> io::Result<String> {
    match run_control_command(tokens) {
        ProcessOutcome::Completed { status, output } if status.success() => Ok(output),
        ProcessOutcome::Completed { status, .. } => Err(resource_error(&format!(
            "systemd control command failed ({status})"
        ))),
        ProcessOutcome::TimedOut { .. } => Err(resource_error("systemd control command timed out")),
        ProcessOutcome::Failed { message, .. } => Err(resource_error(&message)),
    }
}

fn resource_error(message: &str) -> io::Error {
    io::Error::other(format!("mutation resource guard: {message}"))
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "linux_failure_tests.rs"]
mod failure_tests;
