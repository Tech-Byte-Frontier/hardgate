use super::FileCoverage;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Deterministic index of report paths relative to one repository root.
///
/// A normalized key may have multiple report candidates (for example an
/// absolute and relative spelling of the same package path). Such a key is
/// deliberately ambiguous and never resolves to a coverage record.
pub(crate) struct CoveragePathIndex {
    candidates: BTreeMap<String, Vec<PathBuf>>,
    root_key: String,
}

impl CoveragePathIndex {
    pub(crate) fn new(map: &HashMap<PathBuf, FileCoverage>, root: &Path) -> Self {
        let root_key = normalized_root(root);
        let mut candidates = BTreeMap::<String, Vec<PathBuf>>::new();
        for path in map.keys() {
            if let Some(key) = normalized_repository_key(path, root) {
                candidates.entry(key).or_default().push(path.clone());
            }
        }
        for values in candidates.values_mut() {
            values.sort();
        }
        Self {
            candidates,
            root_key,
        }
    }

    pub(crate) fn resolve<'a>(
        &self,
        map: &'a HashMap<PathBuf, FileCoverage>,
        path: &Path,
    ) -> Option<(&'a PathBuf, &'a FileCoverage)> {
        let key = normalized_repository_key_with_root(path, &self.root_key)?;
        let candidates = self.candidates.get(&key)?;
        if candidates.len() != 1 {
            return None;
        }
        let candidate = &candidates[0];
        map.get_key_value(candidate)
    }

    pub(crate) fn key(&self, path: &Path) -> Option<String> {
        normalized_repository_key_with_root(path, &self.root_key)
    }

    pub(crate) fn is_ambiguous(&self, path: &Path) -> bool {
        self.key(path)
            .and_then(|key| self.candidates.get(&key))
            .is_some_and(|values| values.len() > 1)
    }

    pub(crate) fn unique_records<'a>(
        &self,
        map: &'a HashMap<PathBuf, FileCoverage>,
    ) -> Vec<&'a FileCoverage> {
        self.candidates
            .values()
            .filter_map(|paths| {
                if paths.len() == 1 {
                    map.get(&paths[0])
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Normalize a path to an exact repository-relative key.
pub(crate) fn normalized_repository_key(path: &Path, root: &Path) -> Option<String> {
    let root_key = normalized_root(root);
    normalized_repository_key_with_root(path, &root_key)
}

fn normalized_repository_key_with_root(path: &Path, root_key: &str) -> Option<String> {
    let raw = path.to_string_lossy().replace('\\', "/");
    let normalized = normalize_components(raw.as_str())?;
    if is_absolute_text(raw.as_str()) {
        let root_parts = split_components(root_key);
        let candidate_parts = split_components(&normalized);
        if !has_prefix(&candidate_parts, &root_parts) {
            return None;
        }
        return join_components(&candidate_parts[root_parts.len()..]);
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalized_root(root: &Path) -> String {
    let absolute = root
        .canonicalize()
        .ok()
        .or_else(|| root.is_absolute().then(|| root.to_path_buf()));
    let raw = absolute
        .as_deref()
        .unwrap_or(root)
        .to_string_lossy()
        .replace('\\', "/");
    normalize_components(raw.as_str()).unwrap_or_default()
}

fn normalize_components(raw: &str) -> Option<String> {
    let absolute = is_absolute_text(raw);
    let mut parts = Vec::new();
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            value => parts.push(value.to_string()),
        }
    }
    let joined = parts.join("/");
    if absolute {
        Some(if joined.is_empty() {
            "/".to_string()
        } else {
            format!("/{joined}")
        })
    } else {
        Some(joined)
    }
}

fn split_components(path: &str) -> Vec<&str> {
    path.trim_start_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .collect()
}

fn join_components(parts: &[&str]) -> Option<String> {
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn has_prefix(candidate: &[&str], prefix: &[&str]) -> bool {
    candidate.starts_with(prefix)
}

fn is_absolute_text(raw: &str) -> bool {
    raw.starts_with('/') || raw.as_bytes().get(1) == Some(&b':')
}
