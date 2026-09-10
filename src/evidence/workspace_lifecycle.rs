//! Managed job metadata is outside the child-writable source copy. Only the
//! creating owner removes its job; no scan or age-based deletion occurs here.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Lifecycle {
    schema_version: u32,
    owner: String,
    pid: u32,
    source: PathBuf,
    workspace: PathBuf,
    status: String,
    created_unix_secs: u64,
    updated_unix_secs: u64,
    detail: String,
}

pub(super) struct ManagedJob {
    pub root: PathBuf,
    directory: File,
    lock: File,
    state: RefCell<Lifecycle>,
}

impl ManagedJob {
    pub fn create(root: PathBuf, source: &Path) -> Result<Self> {
        let directory = File::open(&root)?;
        let lock = private_file(&root.join(".lock"))?;
        lock.try_lock()
            .context("cannot lock the new Hardgate workspace")?;
        // Descendants retain the lock if the CLI is killed abruptly. The
        // lifecycle remains active until the owner completes publication.
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        rustix::io::fcntl_setfd(&lock, rustix::io::FdFlags::empty())?;
        let now = timestamp();
        let job = Self {
            root: root.clone(),
            directory,
            lock,
            state: RefCell::new(Lifecycle {
                schema_version: 1,
                owner: "hardgate".into(),
                pid: std::process::id(),
                source: source.into(),
                workspace: root.join("work"),
                status: "active".into(),
                created_unix_secs: now,
                updated_unix_secs: now,
                detail: "copying isolated inputs".into(),
            }),
        };
        job.status("active", "copying isolated inputs")?;
        Ok(job)
    }

    pub fn status(&self, status: &str, detail: &str) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.status = status.into();
        state.updated_unix_secs = timestamp();
        state.detail = detail.into();
        let bytes = serde_json::to_vec_pretty(&*state)?;
        drop(state);
        self.verify_owner()?;
        let path = self.root.join("lifecycle.json");
        let mut file = if path.exists() {
            existing_file(&path, false)?
        } else {
            private_file(&path)?
        };
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    }

    pub fn current_status(&self) -> String {
        self.state.borrow().status.clone()
    }

    pub fn verify_owner(&self) -> Result<()> {
        let path = fs::symlink_metadata(&self.root)?;
        ensure!(
            path.is_dir() && !path.is_symlink(),
            "managed workspace path is no longer its owned directory"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let opened = self.directory.metadata()?;
            ensure!(
                path.dev() == opened.dev()
                    && path.ino() == opened.ino()
                    && path.uid() == opened.uid(),
                "managed workspace identity changed; refusing cleanup"
            );
            let current = fs::symlink_metadata(self.root.join(".lock"))?;
            let held = self.lock.metadata()?;
            ensure!(
                !current.is_symlink() && current.ino() == held.ino() && current.dev() == held.dev(),
                "managed workspace lock identity changed; refusing cleanup"
            );
        }
        Ok(())
    }

    pub fn diagnostics(&self, stage: &str, output: &str) -> Result<()> {
        self.verify_owner()?;
        let path = self.root.join("diagnostics.log");
        let mut file = if path.exists() {
            existing_file(&path, true)?
        } else {
            private_file(&path)?
        };
        writeln!(file, "stage={stage}\n{output}")?;
        file.sync_all()?;
        Ok(())
    }

    pub fn completed(&self) -> Result<()> {
        self.status(
            "completed",
            "publication/restoration verified; removing disposable files",
        )?;
        let marker = private_file(&self.root.join(".completed"))?;
        marker.sync_all()?;
        Ok(())
    }

    pub fn report(&self) {
        crate::engines::process::diagnostic(format_args!(
            "hardgate: workspace lifecycle={} job={} workspace={} (preserved)",
            self.current_status(),
            self.root.display(),
            self.root.join("work").display()
        ));
    }
}

fn private_file(path: &Path) -> Result<File> {
    let mut options = File::options();
    options.write(true).read(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn existing_file(path: &Path, append: bool) -> Result<File> {
    let mut options = File::options();
    options.write(true).append(append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "workspace diagnostic must be a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.nlink() == 1,
            "workspace diagnostic must not have aliases"
        );
    }
    if !append {
        file.set_len(0)?;
    }
    Ok(file)
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}
