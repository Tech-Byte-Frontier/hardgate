use crate::resources::runtime::error;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub(super) struct Readiness(pub(super) PathBuf);

impl Readiness {
    pub(super) fn create(identity: &str) -> io::Result<Self> {
        let directory = std::env::temp_dir().join(format!("hardgate-workload-ready-{identity}"));
        DirBuilder::new().mode(0o700).create(&directory)?;
        Ok(Self(directory.join("ready")))
    }

    pub(super) fn observed(&self) -> bool {
        self.0.is_file()
    }
}

impl Drop for Readiness {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        if let Some(parent) = self.0.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

pub(super) fn acknowledge() -> io::Result<()> {
    let Some(marker) = std::env::var_os(super::super::CHILD_MARKER) else {
        return Ok(());
    };
    acknowledge_path(Path::new(&marker))
}

fn acknowledge_path(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| error("invalid workload readiness path"))?;
    let metadata = fs::symlink_metadata(parent)?;
    let uid = rustix::process::getuid().as_raw();
    if path.file_name() != Some(std::ffi::OsStr::new("ready"))
        || !parent.file_name().is_some_and(|name| {
            name.to_string_lossy()
                .starts_with("hardgate-workload-ready-")
        })
        || !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.mode() & 0o7777 != 0o700
    {
        return Err(error("workload readiness directory is not owner-validated"));
    }
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(cause) if cause.kind() == io::ErrorKind::AlreadyExists => validate_existing(path, uid),
        Err(cause) => Err(cause),
    }
}

fn validate_existing(path: &Path, uid: u32) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != 0o600
    {
        return Err(error("invalid inherited workload readiness marker"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "supervisor-readiness_tests.rs"]
mod tests;
