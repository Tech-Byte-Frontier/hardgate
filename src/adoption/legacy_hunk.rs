use crate::engines::{
    BudgetViolation, CloneViolation, ComplexityViolation, DeadCodeViolation, InvariantViolation,
    SuppressionViolation,
};
use crate::git_evidence::ChangeSet;
use std::path::Path;

pub(super) trait LegacyFinding {
    fn attributable(&self, changes: &ChangeSet) -> bool;
}

impl LegacyFinding for BudgetViolation {
    fn attributable(&self, changes: &ChangeSet) -> bool {
        changed_file(&self.file, changes)
    }
}

impl LegacyFinding for SuppressionViolation {
    fn attributable(&self, changes: &ChangeSet) -> bool {
        changed_line(&self.file, self.line_number, changes)
    }
}

impl LegacyFinding for ComplexityViolation {
    fn attributable(&self, changes: &ChangeSet) -> bool {
        changed_line(&self.file, self.line_number, changes)
    }
}

impl LegacyFinding for InvariantViolation {
    fn attributable(&self, changes: &ChangeSet) -> bool {
        changed_line(&self.file, self.line_number, changes)
    }
}

impl LegacyFinding for CloneViolation {
    fn attributable(&self, changes: &ChangeSet) -> bool {
        changed_range(&self.file_a, self.lines_a, changes)
            || changed_range(&self.file_b, self.lines_b, changes)
    }
}

impl LegacyFinding for DeadCodeViolation {
    fn attributable(&self, changes: &ChangeSet) -> bool {
        self.line_number
            .map(|line| changed_line(&self.file, line, changes))
            .unwrap_or_else(|| changed_file(&self.file, changes))
    }
}

fn changed_file(path: &Path, changes: &ChangeSet) -> bool {
    changes.changed_files.contains(path) || changes.changed_lines.contains_key(path)
}

fn changed_line(path: &Path, line: usize, changes: &ChangeSet) -> bool {
    changes
        .changed_lines
        .get(path)
        .is_some_and(|lines| lines.contains(&line))
}

fn changed_range(path: &Path, range: (usize, usize), changes: &ChangeSet) -> bool {
    let Some(lines) = changes.changed_lines.get(path) else {
        return false;
    };
    range.0 <= range.1 && lines.range(range.0..=range.1).next().is_some()
}
