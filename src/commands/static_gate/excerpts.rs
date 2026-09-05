use crate::commands::source_snapshot::SourceSnapshot;
use crate::diagnostics::{GateReport, rules};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Retain only shared bytes referenced by findings. Rendering never rereads disk.
pub(super) fn capture(snapshot: &SourceSnapshot, root: &Path, report: &mut GateReport) {
    let needed: BTreeSet<PathBuf> = rules::diagnostics(report)
        .into_iter()
        .flat_map(|diagnostic| {
            diagnostic
                .locations
                .into_iter()
                .map(|location| location.file)
        })
        .collect();
    for source in &snapshot.files {
        let path = source
            .classified
            .path
            .strip_prefix(root)
            .unwrap_or(&source.classified.path);
        if needed.contains(path)
            && let Ok(content) = &source.content
        {
            report
                .source_text
                .insert(path.to_path_buf(), content.clone());
        }
    }
}
