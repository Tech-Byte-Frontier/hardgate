use crate::config::FileBudgets;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetViolation {
    pub file: PathBuf,
    pub metric: String,
    pub actual: usize,
    pub limit: usize,
    pub message: String,
}

pub fn check_file_budgets(path: &Path, budgets: &FileBudgets, root: &Path) -> Vec<BudgetViolation> {
    let mut violations = Vec::new();

    let rel_path = path.strip_prefix(root).unwrap_or(path);
    let rel_str = rel_path.to_string_lossy();

    // Check if path is in exclusions
    if budgets
        .exclusions
        .paths
        .iter()
        .any(|p| p == rel_str.as_ref())
    {
        return violations;
    }

    // 1. Check max_bytes
    if let Some(max_bytes) = budgets.max_bytes {
        if let Ok(metadata) = fs::metadata(path) {
            let file_size = metadata.len() as usize;
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
    }

    // 2. Check max_lines
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let limit = budgets
        .max_lines
        .get(ext)
        .copied()
        .or_else(|| budgets.max_lines.get("default").copied());

    if let Some(max_lines) = limit {
        if let Ok(content) = fs::read_to_string(path) {
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
    }

    violations
}
