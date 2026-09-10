use crate::config::HardgateConfig;
use crate::engines::{CoverageScorer, coverage::FileCoverage};
use anyhow::{Context, Result, ensure};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(super) fn collect(
    config: &HardgateConfig,
    paths: &[String],
    root: Option<&Path>,
) -> Result<HashMap<PathBuf, FileCoverage>> {
    ensure!(
        !paths.is_empty(),
        "Coverage is enabled, but no report path was provided."
    );
    for path in paths {
        let path = root.map_or_else(|| PathBuf::from(path), |root| root.join(path));
        ensure!(
            path.is_file(),
            "Required coverage report was not found: {}",
            path.display()
        );
    }
    if let Some(root) = root {
        crate::evidence::verify_set(root, paths, crate::evidence::EvidenceKind::Coverage, config)
            .context("Required coverage source identity is invalid")?;
    }
    let scorer = CoverageScorer::new(&config.coverage);
    let mut aggregate = HashMap::new();
    for path in paths {
        let path = root.map_or_else(|| PathBuf::from(path), |root| root.join(path));
        ensure!(
            path.is_file(),
            "Required coverage report was not found: {}",
            path.display()
        );
        let records = match root {
            Some(root) => scorer.parse_lcov_for_project(&path, root, config)?,
            None => scorer.parse_lcov(&path)?,
        };
        for (path, record) in records {
            ensure!(
                aggregate.insert(path.clone(), record).is_none(),
                "overlapping coverage records for {}; partitions must be disjoint",
                path.display()
            );
        }
    }
    Ok(aggregate)
}
