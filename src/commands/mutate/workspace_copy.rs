use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

// Build output and VCS administrative data are not source inputs. Dependencies
// are copied as independent bytes, never hardlinked or linked to live source.
const OMITTED: &[&str] = &[".git", "target"];

pub(super) fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let mut directories = vec![PathBuf::new()];
    while let Some(relative) = directories.pop() {
        crate::cancellation::check()?;
        let mut entries =
            fs::read_dir(source.join(&relative))?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if OMITTED.iter().any(|name| entry.file_name() == *name) {
                continue;
            }
            let path = relative.join(entry.file_name());
            copy_entry(source, destination, &path, &mut directories).with_context(|| {
                format!(
                    "failed to snapshot `{}` for isolated mutation",
                    path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn copy_entry(
    source: &Path,
    destination: &Path,
    relative: &Path,
    directories: &mut Vec<PathBuf>,
) -> Result<()> {
    crate::cancellation::check()?;
    let origin = source.join(relative);
    let copied = destination.join(relative);
    let metadata = fs::symlink_metadata(&origin)?;
    if metadata.is_symlink() {
        return copy_link(source, destination, relative);
    }
    if metadata.is_dir() {
        fs::create_dir(&copied)?;
        directories.push(relative.to_path_buf());
    } else if metadata.is_file() {
        fs::copy(&origin, &copied)?;
        verify_copy(&origin, &copied, &metadata)?;
    } else {
        bail!(
            "unsupported special file in mutation snapshot: {}",
            origin.display()
        );
    }
    Ok(())
}

fn verify_copy(origin: &Path, copied: &Path, before: &fs::Metadata) -> Result<()> {
    let after = fs::symlink_metadata(origin)?;
    if !after.is_file() || before.len() != after.len() || before.modified()? != after.modified()? {
        bail!(
            "source changed during mutation snapshot: {}",
            origin.display()
        );
    }
    if fs::read(origin)? != fs::read(copied)? {
        bail!(
            "source bytes changed during mutation snapshot: {}",
            origin.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
fn copy_link(source: &Path, destination: &Path, relative: &Path) -> Result<()> {
    let target = source.join(relative).canonicalize()?;
    let mapped = target.strip_prefix(source).with_context(|| {
        format!("symlink `{}` leaves the workspace; invoke from a root containing its source/dependency target", relative.display())
    })?;
    if mapped
        .components()
        .any(|part| OMITTED.iter().any(|name| part.as_os_str() == *name))
    {
        bail!(
            "symlink `{}` targets omitted build/VCS data",
            relative.display()
        );
    }
    std::os::unix::fs::symlink(destination.join(mapped), destination.join(relative))?;
    Ok(())
}

#[cfg(not(unix))]
fn copy_link(_source: &Path, _destination: &Path, relative: &Path) -> Result<()> {
    bail!(
        "symlink mutation snapshots are unsupported on this platform: {}",
        relative.display()
    )
}
