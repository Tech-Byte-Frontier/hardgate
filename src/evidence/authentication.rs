//! Local execution authentication: the protected child can write neither this
//! registry nor its ancestors. No signing secret is exposed to project code.
//! This protects against producer/report forgery, not a compromised host user.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(super) struct Certification {
    path: PathBuf,
    slot: PathBuf,
    committed: bool,
}

impl Certification {
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for Certification {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.slot);
            if let Err(error) = fs::remove_file(&self.path) {
                crate::engines::process::diagnostic(format_args!(
                    "hardgate: could not revoke incomplete authentication {}: {error}",
                    self.path.display()
                ));
            }
        }
    }
}

pub(super) fn certify(
    bytes: &[u8],
    source: &Path,
    workspace: &Path,
    report: &Path,
) -> Result<Certification> {
    let root = registry(source, true)?;
    ensure!(
        !root.starts_with(workspace),
        "receipt authentication registry must be outside the producer workspace"
    );
    let path = root.join(digest(bytes));
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .context("cannot authenticate completed producer execution")?;
    let guard = Certification {
        path,
        slot: slot(&root, report)?,
        committed: false,
    };
    file.write_all(bytes)?;
    file.sync_all()?;
    let mut marker = File::options()
        .write(true)
        .create_new(true)
        .open(&guard.slot)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        marker.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    marker.write_all(digest(bytes).as_bytes())?;
    marker.sync_all()?;
    verify(bytes, source, report)?;
    Ok(guard)
}

pub(super) fn verify(bytes: &[u8], source: &Path, report: &Path) -> Result<()> {
    (|| {
    let root = registry(source, false)?;
    ensure!(
        fs::read_to_string(slot(&root, report)?)? == digest(bytes),
        "evidence authentication was revoked by a later producer attempt"
    );
    verify_at(&root, bytes)
    })().context("evidence has no matching protected execution authentication; regenerate with hardgate evidence")
}

pub(super) fn revoke(report: &Path, source: &Path) -> Result<()> {
    let root = registry(source, true)?;
    match fs::remove_file(slot(&root, report)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn slot(registry: &Path, report: &Path) -> Result<PathBuf> {
    let identity = super::workspace::resolved_destination(report)?;
    Ok(registry.join(format!("slot-{}", digest(&serde_json::to_vec(&identity)?))))
}

fn verify_at(root: &Path, bytes: &[u8]) -> Result<()> {
    let path = root.join(digest(bytes));
    let mut options = File::options();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    private_metadata(&metadata, false)?;
    ensure!(
        metadata.len() == bytes.len() as u64,
        "authentication record size changed"
    );
    let mut authenticated = Vec::with_capacity(bytes.len());
    file.read_to_end(&mut authenticated)?;
    ensure!(
        authenticated == bytes,
        "receipt differs from authenticated execution"
    );
    Ok(())
}

fn registry(source: &Path, create: bool) -> Result<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .context("receipt authentication requires XDG_STATE_HOME or HOME")?;
    ensure!(state.is_absolute(), "XDG_STATE_HOME must be absolute");
    let state = super::workspace::resolved_destination(&state)?;
    let root = state.join("hardgate/executions-v2");
    ensure!(
        !root.starts_with(source) && !source.starts_with(&root),
        "authentication registry must be outside source inputs"
    );
    if let Some(cache) = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
    {
        let cache = super::workspace::resolved_destination(&cache)?;
        ensure!(
            !root.starts_with(cache),
            "authentication registry must be outside the child-writable Cargo cache"
        );
    }
    if create {
        fs::create_dir_all(&state)?;
    }
    for directory in [state.join("hardgate"), root.clone()] {
        if create {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        let metadata = fs::symlink_metadata(&directory)?;
        ensure!(
            !metadata.is_symlink(),
            "authentication registry must not be a symlink"
        );
        private_metadata(&metadata, true)?;
    }
    Ok(root)
}

fn private_metadata(metadata: &fs::Metadata, directory: bool) -> Result<()> {
    ensure!(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "authentication artifact has the wrong type"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == rustix::process::geteuid().as_raw() && metadata.mode() & 0o077 == 0,
            "authentication artifact must be owned by this user and private"
        );
        ensure!(
            directory || metadata.nlink() == 1,
            "authentication record must not have hardlinks"
        );
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "authentication_tests.rs"]
mod tests;
