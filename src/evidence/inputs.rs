//! Verification inputs stay protected even when Git ignores them. Only known
//! disposable cache records and explicitly declared report artifacts are outputs.
use crate::config::HardgateConfig;
use crate::discovery::{FileRole, classification::PreparedClassifier};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub(super) struct InputPolicy {
    classifier: PreparedClassifier,
    coverage: Option<PathBuf>,
}

impl InputPolicy {
    pub(super) fn new(root: &Path, config: &HardgateConfig) -> Result<Self> {
        let classifier = PreparedClassifier::new(&config.classification)?;
        let coverage = config.coverage.report.as_ref().and_then(|report| {
            let path = Path::new(report);
            if path.is_absolute() {
                path.strip_prefix(root).ok().map(Path::to_path_buf)
            } else {
                Some(path.to_path_buf())
            }
        });
        let coverage = coverage.filter(|path| {
            let classified = classifier.classify(path);
            path.components().all(|part| {
                matches!(
                    part,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            }) && matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("lcov" | "info")
            ) && matches!(
                classified.role,
                FileRole::Unknown | FileRole::Generated | FileRole::Vendor
            )
        });
        let coverage = coverage.map(|path| {
            path.components()
                .filter(|part| !matches!(part, std::path::Component::CurDir))
                .collect()
        });
        Ok(Self {
            classifier,
            coverage,
        })
    }

    pub(super) fn is_output(&self, path: &Path) -> bool {
        if let Some(report) = &self.coverage {
            let receipt = PathBuf::from(format!("{}.hardgate.json", report.display()));
            if path == report || path == receipt {
                return true;
            }
        }
        is_cache_record(path)
            && !self
                .classifier
                .classify(path)
                .reason
                .starts_with("custom classification rule")
    }
}

fn is_cache_record(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if name == ".eslintcache" {
        return true;
    }
    let parts = path
        .iter()
        .filter_map(|part| part.to_str())
        .collect::<Vec<_>>();
    if parts.contains(&"__pycache__") && path.extension().is_some_and(|ext| ext == "pyc") {
        return true;
    }
    for (position, component) in parts.iter().enumerate() {
        let rest = &parts[position + 1..];
        if let Some(record) = tool_cache_record(component, rest) {
            return record;
        }
    }
    false
}

fn tool_cache_record(component: &str, rest: &[&str]) -> Option<bool> {
    let record = match component {
        ".ruff_cache" => {
            cache_marker(rest)
                || (rest.len() == 2 && rest[1].bytes().all(|byte| byte.is_ascii_hexdigit()))
        }
        ".import_linter_cache" => import_cache_record(rest),
        ".pytest_cache" => pytest_cache_record(rest),
        _ => return None,
    };
    Some(record)
}

fn import_cache_record(rest: &[&str]) -> bool {
    cache_marker(rest)
        || (rest.len() == 1 && (rest[0].ends_with(".data.json") || rest[0].ends_with(".meta.json")))
}

fn pytest_cache_record(rest: &[&str]) -> bool {
    cache_marker(rest)
        || rest == ["README.md"]
        || (rest.len() == 3
            && rest[..2] == ["v", "cache"]
            && matches!(rest[2], "nodeids" | "lastfailed" | "stepwise" | "durations"))
}

fn cache_marker(parts: &[&str]) -> bool {
    parts.len() == 1 && matches!(parts[0], "CACHEDIR.TAG" | ".gitignore")
}
