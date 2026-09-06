use super::{CoverageScorer, FileCoverage, lcov, normalized_repository_key};
use crate::config::HardgateConfig;
use crate::discovery::rust_ownership::RustOwnership;
use crate::discovery::{DiscoverOptions, classification::PreparedClassifier, discover_paths};
use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

impl CoverageScorer {
    /// Score source-bound Rust evidence without crediting colocated test code.
    pub(crate) fn parse_lcov_for_project(
        &self,
        report: &Path,
        root: &Path,
        config: &HardgateConfig,
    ) -> Result<HashMap<PathBuf, FileCoverage>> {
        let discovered = discover_paths(DiscoverOptions {
            root,
            diff_only: false,
            exclusions: &[],
        })?;
        let classifier = PreparedClassifier::new(&config.classification)?;
        let mut sources = BTreeMap::new();
        for path in discovered
            .files
            .into_iter()
            .filter(|path| RustOwnership::context_path(path))
        {
            let Some(key) = normalized_repository_key(&path, root) else {
                continue;
            };
            let file = classifier.classify(Path::new(&key));
            let text = std::fs::read_to_string(&path)?;
            sources.insert(key, (file, text));
        }
        let ownership = RustOwnership::from_inputs(
            &sources
                .values()
                .map(|(file, text)| (file, text.as_str()))
                .collect::<Vec<_>>(),
        );
        let lines = sources
            .iter()
            .map(|(key, (file, text))| (key.clone(), ownership.line_roles(file, text)))
            .collect::<BTreeMap<_, _>>();
        lcov::parse_report_filtered(
            report,
            self.config.min_function_percent.is_some(),
            self.config.min_branch_percent.is_some(),
            Some(&|path, line| {
                let Some(key) = normalized_repository_key(path, root) else {
                    return Ok(false);
                };
                let Some(roles) = lines.get(&key) else {
                    return Ok(false);
                };
                let role = line
                    .checked_sub(1)
                    .and_then(|index| roles.get(index))
                    .ok_or_else(|| {
                        anyhow::anyhow!("reported line {line} is outside source {key}")
                    })?;
                role.ok_or_else(|| anyhow::anyhow!("line {line} in {key} mixes production and test code; line-only evidence cannot establish ownership"))
            }),
        )
    }
}

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod tests;
