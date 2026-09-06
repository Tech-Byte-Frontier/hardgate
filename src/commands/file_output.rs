use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_OUTPUT: AtomicU64 = AtomicU64::new(0);

struct TemporaryOutput(PathBuf);

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn write_atomic_file(path: &Path, content: &str) -> Result<()> {
    crate::resources::runtime::verify_active()?;
    let name = path.file_name().context("output path must name a file")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create output directory `{}`", parent.display()))?;
    for _ in 0..32 {
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(
            ".hardgate-output-{}-{}",
            std::process::id(),
            NEXT_OUTPUT.fetch_add(1, Ordering::Relaxed)
        ));
        let temporary_path = parent.join(temporary_name);
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("Failed to create temporary output file"),
        };
        let temporary = TemporaryOutput(temporary_path);
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        crate::resources::runtime::verify_active()?;
        fs::rename(&temporary.0, path)
            .with_context(|| format!("Failed to replace output file `{}`", path.display()))?;
        return Ok(());
    }
    anyhow::bail!(
        "Unable to allocate a unique temporary output file for `{}`",
        path.display()
    )
}

#[cfg(test)]
#[path = "file_output_tests.rs"]
mod tests;

/// Resolve existing parent links and lexical dot components before writing either output.
pub(crate) fn same_output_path(left: &Path, right: &Path) -> Result<bool> {
    fn identity(path: &Path) -> Result<PathBuf> {
        let absolute = std::env::current_dir()?.join(path);
        let ancestor = absolute
            .ancestors()
            .find(|path| path.exists())
            .context("output path has no existing ancestor")?;
        let mut resolved = ancestor.canonicalize()?;
        for part in absolute.strip_prefix(ancestor)?.components() {
            match part {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    resolved.pop();
                }
                _ => resolved.push(part.as_os_str()),
            }
        }
        Ok(crate::engines::clones::repository_relative_path(
            &resolved,
            Path::new("/"),
        ))
    }
    Ok(identity(left)? == identity(right)?)
}
