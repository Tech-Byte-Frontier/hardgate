use super::detect::ReferenceStatus;
use std::path::Path;
use std::process::Command;

pub(crate) fn legacy_reference_status(root: &Path, branch: &str) -> ReferenceStatus {
    let Some(root) = root.to_str() else {
        return ReferenceStatus::Unknown;
    };
    let reference = format!("{branch}^{{commit}}");
    match Command::new("git")
        .args(["-C", root, "rev-parse", "--verify"])
        .arg(&reference)
        .output()
    {
        Ok(output) if output.status.success() => ReferenceStatus::Available,
        Ok(_) => ReferenceStatus::Missing,
        Err(_) => ReferenceStatus::Unknown,
    }
}

#[cfg(test)]
#[path = "reference_tests.rs"]
mod tests;
