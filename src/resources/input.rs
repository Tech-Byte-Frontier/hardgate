use std::fs::File;
use std::io::{self, Read};

const READ_CHUNK_BYTES: usize = 64 * 1024;
const RESOURCE_GUARD_PREFIX: &str = "mutation resource guard";

pub(crate) fn read_source(mut source: File, limit: usize) -> io::Result<Vec<u8>> {
    let metadata_bytes = source.metadata()?.len();
    if metadata_bytes > limit_as_u64(limit) {
        return Err(source_limit_error(format_args!(
            "source length {metadata_bytes} bytes exceeds limit {limit} bytes"
        )));
    }

    let mut bytes = Vec::new();
    let mut total = 0;
    let mut chunk = [0; READ_CHUNK_BYTES];
    loop {
        let read = source.read(&mut chunk)?;
        if read == 0 {
            return Ok(bytes);
        }
        admit_bytes(&mut total, read, limit)?;
        bytes.extend_from_slice(&chunk[..read]);
    }
}

pub(crate) fn admit_bytes(total: &mut usize, additional: usize, limit: usize) -> io::Result<()> {
    let next = total
        .checked_add(additional)
        .ok_or_else(|| source_limit_error(format_args!("byte count overflow at limit {limit}")))?;
    if next > limit {
        return Err(source_limit_error(format_args!(
            "source/snapshot size {next} bytes exceeds limit {limit} bytes"
        )));
    }
    *total = next;
    Ok(())
}

fn limit_as_u64(limit: usize) -> u64 {
    u64::try_from(limit).unwrap_or(u64::MAX)
}

fn source_limit_error(detail: std::fmt::Arguments<'_>) -> io::Error {
    io::Error::other(format!(
        "{RESOURCE_GUARD_PREFIX}: source/snapshot limit exceeded: {detail}"
    ))
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
