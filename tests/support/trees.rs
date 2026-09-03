//! File-tree fixtures for discovery tests.

use std::path::{Path, PathBuf};

/// Write `rel/paths` under `root` (creating parents) with stub content.
/// Returns the absolute paths in order.
pub fn write_tree(root: &Path, rel_paths: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(rel_paths.len());
    for rel in rel_paths {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&abs, "// fixture\n").unwrap();
        out.push(abs);
    }
    out
}

/// True when any path in `files` ends with the given suffix.
pub fn has_suffix(files: &[PathBuf], suffix: &str) -> bool {
    files.iter().any(|f| f.ends_with(suffix))
}
