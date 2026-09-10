use crate::engines::process::ProcessOutcome;
use std::path::{Path, PathBuf};
static MANAGED_ROOT: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

/// An explicit root wins over the inherited TMPDIR. Child scratch remains
/// private to its execution copy and never redirects writes to host source.
pub fn configure_root(root: Option<PathBuf>) -> std::io::Result<()> {
    let selected = root.or_else(|| std::env::var_os("HARDGATE_SCRATCH_ROOT").map(PathBuf::from));
    if selected
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(std::io::Error::other("scratch root must not be empty"));
    }
    MANAGED_ROOT
        .set(selected)
        .map_err(|_| std::io::Error::other("scratch root was already configured"))
}

pub(super) fn managed_root() -> std::io::Result<PathBuf> {
    let selected = MANAGED_ROOT
        .get()
        .cloned()
        .flatten()
        .or_else(|| std::env::var_os("HARDGATE_SCRATCH_ROOT").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    Ok(selected)
}

pub(crate) fn scratch_directory(workspace: &Path) -> PathBuf {
    workspace.join(".hardgate/evidence/tmp")
}

pub(super) fn explain(outcome: ProcessOutcome, workspace: &Path) -> ProcessOutcome {
    match outcome {
        ProcessOutcome::Completed { status, output } if !status.success() => {
            ProcessOutcome::Completed {
                status,
                output: annotate(output, workspace),
            }
        }
        ProcessOutcome::Failed { message, output } => ProcessOutcome::Failed {
            message,
            output: annotate(output, workspace),
        },
        other => other,
    }
}

fn annotate(mut output: String, workspace: &Path) -> String {
    if crate::resources::runtime::isolated()
        && output.contains("/tmp/")
        && (output.contains("Permission denied") || output.contains("Operation not permitted"))
    {
        output.push_str(&format!(
            "\nHardgate containment restricts writes to the disposable workspace. Runtime writable TMPDIR={} (preserved on failure). Use mktemp -d \"${{TMPDIR:-/tmp}}/finance-hardgate.XXXXXXXX\" inside the configured command. A shell outside Hardgate must use its own host/agent TMPDIR.\n",
            scratch_directory(workspace).display()
        ));
    }
    output
}

pub(crate) fn configure(
    command: &mut std::process::Command,
    workspace: &Path,
) -> std::io::Result<()> {
    let scratch = scratch_directory(workspace);
    std::fs::create_dir_all(&scratch)?;
    command
        .env("TMPDIR", &scratch)
        .env("TMP", &scratch)
        .env("TEMP", &scratch)
        .env("HARDGATE_SCRATCH_ROOT", &scratch)
        .env("XDG_CACHE_HOME", scratch.join("cache"))
        .env("XDG_STATE_HOME", scratch.join("state"))
        .env("UV_CACHE_DIR", scratch.join("uv"))
        .env("npm_config_cache", scratch.join("npm"))
        .env("npm_config_store_dir", scratch.join("pnpm-store"))
        .env("pnpm_config_store_dir", scratch.join("pnpm-store"))
        .env("npm_config_verify_deps_before_run", "false")
        .env("pnpm_config_verify_deps_before_run", "false");
    Ok(())
}
