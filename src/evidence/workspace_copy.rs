use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

// Build output and VCS administrative data are not source inputs. Dependencies
// are copied as independent bytes, never hardlinked or linked to live source.

const COPY_BUFFER_SIZE: usize = 64 * 1024;

pub(super) fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let mut directories = vec![PathBuf::new()];
    while let Some(relative) = directories.pop() {
        crate::cancellation::check()?;
        crate::resources::check_pressure()?;
        let mut entries =
            fs::read_dir(source.join(&relative))?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            crate::resources::check_pressure()?;
            let path = relative.join(entry.file_name());
            if super::super::snapshot::omitted(&path, false) {
                continue;
            }
            copy_entry(source, destination, &path, &mut directories).with_context(|| {
                format!(
                    "failed to snapshot `{}` for isolated evidence",
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
        copy_file(&origin, &copied, &metadata.permissions())?;
        verify_copy(&origin, &copied, &metadata)?;
    } else {
        bail!(
            "unsupported special file in evidence snapshot: {}",
            origin.display()
        );
    }
    Ok(())
}

fn copy_file(origin: &Path, copied: &Path, permissions: &fs::Permissions) -> Result<()> {
    let mut input = fs::File::open(origin)?;
    {
        let mut output = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(copied)?;
        let mut buffer = [0_u8; COPY_BUFFER_SIZE];
        loop {
            crate::cancellation::check()?;
            crate::resources::check_pressure()?;
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read])?;
        }
        output.flush()?;
    }
    fs::set_permissions(copied, permissions.clone())?;
    Ok(())
}

fn verify_copy(origin: &Path, copied: &Path, before: &fs::Metadata) -> Result<()> {
    let after = fs::symlink_metadata(origin)?;
    if !after.is_file() || before.len() != after.len() || before.modified()? != after.modified()? {
        bail!(
            "source changed during evidence snapshot: {}",
            origin.display()
        );
    }
    if !files_match(origin, copied, before)? {
        bail!(
            "source bytes changed during evidence snapshot: {}",
            origin.display()
        );
    }
    Ok(())
}

fn files_match(origin: &Path, copied: &Path, before: &fs::Metadata) -> Result<bool> {
    let mut origin_file = fs::File::open(origin)?;
    let mut copied_file = fs::File::open(copied)?;
    let expected_size = before.len();
    if origin_file.metadata()?.len() != expected_size
        || copied_file.metadata()?.len() != expected_size
    {
        return Ok(false);
    }

    let mut origin_buffer = [0_u8; COPY_BUFFER_SIZE];
    let mut copied_buffer = [0_u8; COPY_BUFFER_SIZE];
    let mut remaining = expected_size;
    while remaining > 0 {
        crate::cancellation::check()?;
        crate::resources::check_pressure()?;
        let chunk_size = remaining.min(COPY_BUFFER_SIZE as u64) as usize;
        origin_file.read_exact(&mut origin_buffer[..chunk_size])?;
        copied_file.read_exact(&mut copied_buffer[..chunk_size])?;
        if origin_buffer[..chunk_size] != copied_buffer[..chunk_size] {
            return Ok(false);
        }
        remaining -= chunk_size as u64;
    }

    let context = FileEndContext {
        origin,
        copied,
        before,
    };
    verify_file_end_state(&mut origin_file, &mut copied_file, &context)
}

struct FileEndContext<'a> {
    origin: &'a Path,
    copied: &'a Path,
    before: &'a fs::Metadata,
}

fn verify_file_end_state(
    origin_file: &mut fs::File,
    copied_file: &mut fs::File,
    context: &FileEndContext<'_>,
) -> Result<bool> {
    let expected_size = context.before.len();
    let mut origin_extra = [0_u8; 1];
    let mut copied_extra = [0_u8; 1];
    let origin_read = origin_file.read(&mut origin_extra)?;
    let copied_read = copied_file.read(&mut copied_extra)?;
    if origin_read != 0 || copied_read != 0 {
        return Ok(false);
    }

    let origin_after = fs::symlink_metadata(context.origin)?;
    let copied_after = fs::symlink_metadata(context.copied)?;
    if !origin_after.is_file()
        || !copied_after.is_file()
        || origin_after.len() != expected_size
        || origin_after.modified()? != context.before.modified()?
        || copied_after.len() != expected_size
    {
        return Ok(false);
    }
    Ok(true)
}

#[cfg(unix)]
fn copy_link(source: &Path, destination: &Path, relative: &Path) -> Result<()> {
    let target = source.join(relative).canonicalize()?;
    if !target.starts_with(source)
        && let Some(interpreter) = super::super::environment::interpreter_target(source, relative)?
    {
        std::os::unix::fs::symlink(interpreter, destination.join(relative))?;
        return Ok(());
    }
    let mapped = target.strip_prefix(source).with_context(|| {
        format!("symlink `{}` leaves the workspace; invoke from a root containing its source/dependency target", relative.display())
    })?;
    if super::super::snapshot::omitted(mapped, false) {
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
        "symlink evidence snapshots are unsupported on this platform: {}",
        relative.display()
    )
}
