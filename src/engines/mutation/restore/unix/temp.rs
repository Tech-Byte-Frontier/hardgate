use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

const TEMP_ATTEMPTS: u64 = 32;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn create_temp_file(parent: &File, name: &OsStr) -> io::Result<(OsString, File)> {
    let seed = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    for offset in 0..TEMP_ATTEMPTS {
        let temp_name = OsString::from(format!(
            ".{}.hardgate-{}-{}.tmp",
            name.to_string_lossy(),
            std::process::id(),
            seed.saturating_add(offset)
        ));
        match rustix::fs::openat(
            parent,
            &temp_name,
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0o600),
        ) {
            Ok(fd) => return Ok((temp_name, fd.into())),
            Err(error) if io::Error::from(error).kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io::Error::from(error)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "could not allocate a temporary mutation file beside '{}'; tried {TEMP_ATTEMPTS} names",
            name.to_string_lossy()
        ),
    ))
}
