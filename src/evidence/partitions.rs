use super::{EvidenceKind, EvidenceOptions};
use crate::config::{ConfigContext, HardgateConfig, ProducerConfig};
use crate::discovery::{
    ClassifiedFile, DiscoverOptions, FileRole, classification::PreparedClassifier, discover_paths,
    rust_ownership::RustOwnership,
};
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Partition {
    pub name: String,
    pub config: ProducerConfig,
    pub sources: Vec<PathBuf>,
}

pub(super) fn resolve(
    options: &mut EvidenceOptions,
    context: &ConfigContext,
) -> Result<Option<Partition>> {
    let Some(name) = options.producer_config.as_ref() else {
        return Ok(None);
    };
    let config = context
        .config
        .evidence
        .producers
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("unknown evidence producer configuration `{name}`"))?;
    ensure!(
        config.producer == options.producer,
        "named configuration `{name}` selects a different producer"
    );
    ensure!(
        options.args.is_empty() && options.toolchain.is_none() && options.name.is_none(),
        "named producer configuration owns args, toolchain and artifact name; remove conflicting CLI overrides"
    );
    options.args = config.args.clone();
    options.toolchain = config.toolchain.clone();
    options.name = Some(name.clone());
    // A CLI deadline can shorten but never silently extend the configured deadline.
    options.timeout_secs = options.timeout_secs.min(config.timeout_secs);
    let sources = selected_sources(&context.root, &context.config, config)?;
    ensure!(
        !sources.is_empty(),
        "evidence partition `{name}` has no applicable source inputs"
    );
    for path in &sources {
        ensure!(
            supports(config.producer, path),
            "producer {:?} cannot instrument executable source {}; choose a matching producer partition",
            config.producer,
            path.display()
        );
    }
    Ok(Some(Partition {
        name: name.clone(),
        config: config.clone(),
        sources,
    }))
}

fn supports(producer: super::Producer, path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    match producer {
        super::Producer::CargoLlvmCov | super::Producer::CargoMutants => extension == "rs",
        super::Producer::Pytest => extension == "py",
        super::Producer::Vitest | super::Producer::Stryker => matches!(
            extension,
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts"
        ),
    }
}

pub(super) fn source_inventory(root: &Path, config: &HardgateConfig) -> Result<Vec<PathBuf>> {
    let classifier = PreparedClassifier::new(&config.classification)?;
    let ownership = RustOwnership::from_root(root, config, &[])?;
    let paths = discover_paths(DiscoverOptions {
        root,
        diff_only: false,
        exclusions: &[],
    })?;
    let mut sources = Vec::new();
    for path in paths.files {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let mut file = classifier.classify(relative);
        file.path = path.clone();
        if file.role == FileRole::Source
            && !crate::engines::coverage::applicability::execution_not_applicable(&path)
        {
            if path.extension().is_some_and(|extension| extension == "rs")
                && !rust_execution_source(&file, root, &ownership)?
            {
                continue;
            }
            sources.push(relative.to_path_buf());
        }
    }
    sources.sort();
    sources.dedup();
    Ok(sources)
}

fn rust_execution_source(
    file: &ClassifiedFile,
    root: &Path,
    ownership: &RustOwnership,
) -> Result<bool> {
    let content = std::fs::read_to_string(&file.path)?;
    for view in ownership.views(file, &content) {
        if view.file.role != FileRole::Source {
            continue;
        }
        let structure = crate::engines::ComplexityAnalyzer::new().analyze_role_structure(
            &file.path,
            (&content, &view.text),
            root,
        )?;
        if !structure.functions.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn selected_sources(
    root: &Path,
    config: &HardgateConfig,
    producer: &ProducerConfig,
) -> Result<Vec<PathBuf>> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in &producer.sources {
        builder.add(globset::Glob::new(pattern)?);
    }
    let globs = builder.build()?;
    Ok(source_inventory(root, config)?
        .into_iter()
        .filter(|path| globs.is_match(path))
        .collect())
}

pub fn reports(config: &HardgateConfig, kind: EvidenceKind) -> Vec<String> {
    let named: Vec<_> = config
        .evidence
        .producers
        .iter()
        .filter(|(_, entry)| entry.producer.kind() == kind)
        .map(|(name, _)| {
            format!(
                ".hardgate/evidence/{name}.{}",
                if kind == EvidenceKind::Coverage {
                    "lcov"
                } else {
                    "json"
                }
            )
        })
        .collect();
    if !named.is_empty() {
        return named;
    }
    match kind {
        EvidenceKind::Coverage => config.coverage.report.iter().cloned().collect(),
        EvidenceKind::Mutation => config.mutation.reports.clone().unwrap_or_default(),
    }
}

#[cfg(test)]
#[path = "partitions_tests.rs"]
mod tests;
