use crate::discovery::ClassifiedFile;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) type SharedSource = (PathBuf, Arc<str>);

/// IDs are deterministic within one lexically ordered immutable capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FileId(pub usize);

pub(crate) struct SourceFile {
    pub id: FileId,
    pub classified: ClassifiedFile,
    pub content: Result<Arc<str>, String>,
}

/// Every engine uses these captured bytes. This is not an atomic filesystem
/// transaction: concurrent edits between files can affect capture, but cannot
/// make downstream engines inspect different versions of the same file.
#[derive(Default)]
pub(crate) struct SourceSnapshot {
    pub files: Vec<SourceFile>,
}

impl SourceSnapshot {
    pub fn from_shared(mut inputs: Vec<(ClassifiedFile, Arc<str>)>) -> Self {
        inputs.sort_by(|(left, _), (right, _)| left.path.cmp(&right.path));
        inputs.dedup_by(|(left, _), (right, _)| left.path == right.path);
        let files = inputs
            .into_iter()
            .enumerate()
            .map(|(id, (classified, text))| SourceFile {
                id: FileId(id),
                classified,
                content: Ok(text),
            })
            .collect();
        Self { files }
    }

    pub fn capture(mut files: Vec<ClassifiedFile>) -> Self {
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files.dedup_by(|left, right| left.path == right.path);
        let read = |(id, classified): (usize, ClassifiedFile)| SourceFile {
            id: FileId(id),
            content: std::fs::read_to_string(&classified.path)
                .map(Arc::from)
                .map_err(|error| error.to_string()),
            classified,
        };
        let files = if files.len() < 8 {
            files.into_iter().enumerate().map(read).collect()
        } else {
            files.into_par_iter().enumerate().map(read).collect()
        };
        Self { files }
    }

    pub fn find(&self, path: &Path) -> Option<&SourceFile> {
        let index = self
            .files
            .binary_search_by(|file| file.classified.path.as_path().cmp(path))
            .ok()?;
        self.files.get(index)
    }

    pub fn shared_contents(&self, paths: &[PathBuf]) -> Vec<SharedSource> {
        paths
            .iter()
            .filter_map(|path| {
                let file = self.find(path)?;
                file.content
                    .as_ref()
                    .ok()
                    .map(|text| (path.clone(), Arc::clone(text)))
            })
            .collect()
    }

    pub fn selected_ids(&self, paths: &[PathBuf]) -> Vec<FileId> {
        paths
            .iter()
            .filter_map(|path| self.find(path).map(|file| file.id))
            .collect()
    }
}
