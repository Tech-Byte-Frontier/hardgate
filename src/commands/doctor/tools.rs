use super::Check;
use std::path::{Path, PathBuf};

pub(super) fn tool_check(name: &str, command: &str, root: &Path) -> Check {
    let tokens = crate::engines::orchestration::shell_words_split(command);
    let resolved = tokens.first().and_then(|program| resolve(program, root));
    let dependency = delegated_check(&tokens, root);
    if let Err(error) = dependency {
        return Check {
            name: name.into(),
            ready: false,
            detail: format!("{error:#}; install project dependencies or correct `{command}`"),
        };
    }
    Check {
        name: name.into(),
        ready: resolved.is_some(),
        detail: resolved.map_or_else(
            || {
                format!(
                    "launcher unavailable for `{command}`; install it or correct [orchestration]"
                )
            },
            |path| {
                format!(
                    "launcher {} for `{command}` (arguments/scripts unverified)",
                    path.display()
                )
            },
        ),
    }
}

fn resolve(program: &str, root: &Path) -> Option<PathBuf> {
    let candidates = if program.contains('/') {
        vec![root.join(program)]
    } else {
        std::iter::once(root.join("node_modules/.bin"))
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            ))
            .map(|directory| root.join(directory).join(program))
            .collect()
    };
    candidates.into_iter().find(|path| {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    })
}

fn delegated_check(tokens: &[String], root: &Path) -> anyhow::Result<()> {
    let Some(program) = tokens
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|name| name.to_str())
    else {
        return Ok(());
    };
    if !matches!(program, "npm" | "pnpm" | "yarn") {
        return Ok(());
    }
    match tokens.get(1).map(String::as_str) {
        Some("exec") => {
            if let Some(tool) = tokens.get(2).filter(|tool| !tool.starts_with('-')) {
                anyhow::ensure!(
                    resolve(tool, root).is_some(),
                    "delegated tool `{tool}` is unavailable locally; doctor will not download it"
                );
            }
        }
        Some("run") => {
            if let Some(script) = tokens.get(2).filter(|script| !script.starts_with('-')) {
                let manifest: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(root.join("package.json"))?)?;
                anyhow::ensure!(
                    manifest["scripts"][script]
                        .as_str()
                        .is_some_and(|body| !body.trim().is_empty()),
                    "package.json has no non-empty `{script}` script"
                );
            }
        }
        _ => {}
    }
    Ok(())
}
