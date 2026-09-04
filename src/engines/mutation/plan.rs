use super::super::js::{ResolvedTestPlan, TestSelection};
use crate::engines::process::CommandRoots;
use std::path::Path;

pub(super) fn custom_plan(command: &str, file: &Path, root: &Path) -> ResolvedTestPlan {
    let stem = file
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("");
    let command = command
        .replace("{file}", &file.to_string_lossy())
        .replace("{stem}", stem);
    plain_plan(command, root, TestSelection::Custom)
}

pub(super) fn rust_plan(file: &Path, root: &Path) -> ResolvedTestPlan {
    let stem = file
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("");
    let command = if stem.is_empty() || matches!(stem, "main" | "lib" | "mod") {
        "cargo test".to_string()
    } else {
        format!("cargo test {stem}")
    };
    plain_plan(command, root, TestSelection::Custom)
}

pub(super) fn plain_plan(
    command: String,
    root: &Path,
    selection: TestSelection,
) -> ResolvedTestPlan {
    ResolvedTestPlan {
        command,
        working_dir: root.to_path_buf(),
        package_root: root.to_path_buf(),
        workspace_root: root.to_path_buf(),
        manager: None,
        framework: None,
        selection,
        recommended_timeout_secs: super::DEFAULT_TIMEOUT_SECS,
    }
}

pub(super) fn process_roots(plan: &ResolvedTestPlan) -> CommandRoots<'_> {
    CommandRoots {
        working_dir: &plan.working_dir,
        package_root: &plan.package_root,
        workspace_root: &plan.workspace_root,
    }
}
