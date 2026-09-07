//! Recognize virtualenv interpreter links as installed tools, without granting
//! a general exception for symlinks outside the protected project input tree.
use anyhow::{Context, Result, ensure};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn interpreter_target(root: &Path, relative: &Path) -> Result<Option<PathBuf>> {
    let Some(bin) = relative
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "bin"))
    else {
        return Ok(None);
    };
    let Some(environment) = bin.parent().filter(|path| environment_name(path)) else {
        return Ok(None);
    };
    let Some(name) = relative.file_name().and_then(|name| name.to_str()) else {
        return Ok(None);
    };
    if !python_name(name) {
        return Ok(None);
    }
    let marker = root.join(environment).join("pyvenv.cfg");
    let Ok(metadata) = fs::symlink_metadata(&marker) else {
        return Ok(None);
    };
    ensure!(
        metadata.is_file() && !metadata.is_symlink(),
        "virtualenv pyvenv.cfg must be a regular input file"
    );
    let config = fs::read_to_string(marker)?;
    let home = config
        .lines()
        .filter_map(|line| line.split_once('='))
        .find_map(|(key, value)| (key.trim() == "home").then_some(value.trim()));
    let Some(home) = home else {
        return Ok(None);
    };
    let home = Path::new(home);
    ensure!(
        home.is_absolute(),
        "virtualenv interpreter home must be absolute"
    );
    let target = root
        .join(relative)
        .canonicalize()
        .with_context(|| format!("virtualenv interpreter is missing: {}", relative.display()))?;
    let expected = home.join(name).canonicalize();
    let allowed = expected.is_ok_and(|expected| target == expected)
        || ["python", "python3"].iter().any(|name| {
            home.join(name)
                .canonicalize()
                .is_ok_and(|expected| target == expected)
        });
    ensure!(
        allowed
            && target
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(python_name),
        "virtualenv interpreter link does not match its declared runtime: {}",
        relative.display()
    );
    let metadata = fs::metadata(&target)?;
    ensure!(
        metadata.is_file(),
        "virtualenv interpreter is not a regular executable"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            metadata.permissions().mode() & 0o111 != 0,
            "virtualenv interpreter is not executable"
        );
    }
    Ok(Some(target))
}

fn environment_name(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == ".venv" || name == "venv")
        || path
            .parent()
            .is_some_and(|parent| parent.file_name().is_some_and(|name| name == ".tox"))
}

fn python_name(name: &str) -> bool {
    name.strip_prefix("python").is_some_and(|suffix| {
        suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    })
}

#[cfg(all(test, unix))]
#[path = "environment_tests.rs"]
mod tests;
