use crate::config::FileBudgets;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// One file that breached a byte or physical-line budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetViolation {
    pub file: PathBuf,
    pub metric: String,
    pub actual: usize,
    pub limit: usize,
    pub message: String,
}

pub fn check_file_budgets(path: &Path, budgets: &FileBudgets, root: &Path) -> Vec<BudgetViolation> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    check_content_budgets(path, &content, budgets, root)
}

/// Check an already-loaded file, allowing Git snapshots to use the exact
/// same byte/line policy without materializing historical blobs on disk.
pub fn check_content_budgets(
    path: &Path,
    content: &str,
    budgets: &FileBudgets,
    root: &Path,
) -> Vec<BudgetViolation> {
    let mut violations = Vec::new();

    let rel_path = path.strip_prefix(root).unwrap_or(path);
    let rel_str = rel_path.to_string_lossy();

    // Check if path is in exclusions (exact match for back-compat + glob for patterns like src/generated/**)
    if is_budget_excluded(rel_path, &rel_str, &budgets.exclusions.paths) {
        return violations;
    }

    if let Some(max_bytes) = budgets.max_bytes {
        let file_size = content.len();
        if file_size > max_bytes as usize {
            violations.push(BudgetViolation {
                file: rel_path.to_path_buf(),
                metric: "File Byte Size".to_string(),
                actual: file_size,
                limit: max_bytes as usize,
                message: format!(
                    "File size {} bytes exceeds hard limit of {} bytes ({:.1} KiB)",
                    file_size,
                    max_bytes,
                    max_bytes as f64 / 1024.0
                ),
            });
        }
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let limit = budgets
        .max_lines
        .get(ext)
        .copied()
        .or_else(|| budgets.max_lines.get("default").copied());

    if let Some(max_lines) = limit {
        let physical_lines = content.lines().count();
        if physical_lines > max_lines {
            violations.push(BudgetViolation {
                file: rel_path.to_path_buf(),
                metric: format!("Physical Lines (.{})", ext),
                actual: physical_lines,
                limit: max_lines,
                message: format!(
                    "File has {} physical lines, exceeding budget of {} lines for .{}",
                    physical_lines, max_lines, ext
                ),
            });
        }
    }

    violations
}

fn is_budget_excluded(rel_path: &Path, rel_str: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return false;
    }
    // Fast path: exact match (back-compat with existing configs/tests).
    if patterns.iter().any(|p| p == rel_str) {
        return true;
    }
    let mut builder = GlobSetBuilder::new();
    let mut has_glob = false;
    for p in patterns {
        if let Ok(g) = Glob::new(p) {
            builder.add(g);
            has_glob = true;
        }
    }
    if !has_glob {
        return false;
    }
    let set = builder.build().unwrap_or_else(|_| GlobSet::empty());
    set.is_match(rel_path) || set.is_match(rel_str)
}
