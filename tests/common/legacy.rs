use hardgate::git_evidence::{ChangeSet, ChangedLineMap};
use std::path::PathBuf;

pub fn changes(lines: &[(&str, &[usize])], files: &[&str], renames: &[(&str, &str)]) -> ChangeSet {
    let mut changed_lines = ChangedLineMap::new();
    for (path, numbers) in lines {
        changed_lines.insert(PathBuf::from(path), numbers.iter().copied().collect());
    }
    ChangeSet {
        merge_base: "abc123-merge-base".to_string(),
        changed_lines,
        changed_files: files.iter().map(PathBuf::from).collect(),
        rename_lineage: renames
            .iter()
            .map(|(current, baseline)| (PathBuf::from(current), PathBuf::from(baseline)))
            .collect(),
    }
}
