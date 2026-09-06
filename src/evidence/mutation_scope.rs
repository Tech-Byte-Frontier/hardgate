use super::{EvidenceInputs, Producer, Snapshot};
use crate::discovery::rust_ownership::RustOwnership;
use anyhow::{Context, Result, ensure};
use std::path::Path;

pub(super) fn validate(
    value: &serde_json::Value,
    producer: Producer,
    context: &EvidenceInputs<'_>,
) -> Result<()> {
    let inputs = context.snapshot;
    let config = context.config;
    let root = context.root;
    let names: Vec<&str> = if producer == Producer::Stryker {
        value
            .get("files")
            .and_then(serde_json::Value::as_object)
            .context("missing mutation files")?
            .keys()
            .map(String::as_str)
            .collect()
    } else {
        value
            .get("outcomes")
            .and_then(serde_json::Value::as_array)
            .context("missing mutation outcomes")?
            .iter()
            .filter_map(|outcome| outcome.pointer("/scenario/Mutant"))
            .map(|mutant| {
                mutant
                    .get("file")
                    .and_then(serde_json::Value::as_str)
                    .context("cargo-mutants scenario lacks source path")
            })
            .collect::<Result<_>>()?
    };
    let classifier =
        crate::discovery::classification::PreparedClassifier::new(&config.classification)?;
    for name in names {
        let path = Path::new(name);
        ensure!(
            inputs.0.contains_key(path),
            "mutation source is outside executed project inputs: {name}"
        );
        let file = classifier.classify(path);
        ensure!(
            file.role.is_mutation_target()
                && file.ast_supported
                && config
                    .roles
                    .for_role(file.role)
                    .and_then(|role| role.mutation_target)
                    .unwrap_or(true),
            "mutation source is outside configured source-role scope: {name}"
        );
    }
    if producer == Producer::CargoMutants {
        validate_rust(value, inputs, config, root)?;
    }
    Ok(())
}

fn validate_rust(
    value: &serde_json::Value,
    inputs: &Snapshot,
    config: &crate::config::HardgateConfig,
    root: &Path,
) -> Result<()> {
    let classifier =
        crate::discovery::classification::PreparedClassifier::new(&config.classification)?;
    let mut sources = std::collections::BTreeMap::new();
    for path in inputs
        .0
        .keys()
        .filter(|path| RustOwnership::context_path(path))
    {
        let text = std::fs::read_to_string(root.join(path))?;
        sources.insert(path.clone(), (classifier.classify(path), text));
    }
    let ownership = RustOwnership::from_inputs(
        &sources
            .values()
            .map(|(file, text)| (file, text.as_str()))
            .collect::<Vec<_>>(),
    );
    for mutant in value["outcomes"]
        .as_array()
        .context("missing outcomes")?
        .iter()
        .filter_map(|outcome| outcome.pointer("/scenario/Mutant"))
    {
        let name = mutant["file"].as_str().context("missing mutant source")?;
        let (file, text) = sources
            .get(Path::new(name))
            .context("missing bound Rust source")?;
        let start = position(text, &mutant["span"]["start"])?;
        let end = position(text, &mutant["span"]["end"])?;
        ensure!(
            start < end,
            "mutant source span must be non-empty and ordered: {name}"
        );
        ensure!(
            !ownership.test_span(file, start..end),
            "mutation source is test-only Rust code: {name}"
        );
    }
    Ok(())
}

fn position(text: &str, point: &serde_json::Value) -> Result<usize> {
    let line = point["line"]
        .as_u64()
        .context("mutant span lacks one-based line")?;
    let column = point["column"]
        .as_u64()
        .context("mutant span lacks one-based column")?;
    ensure!(
        line > 0 && column > 0,
        "mutant span coordinates must be one-based"
    );
    let mut current_line = 1;
    let mut current_column = 1;
    for (byte, character) in text.char_indices() {
        if (current_line, current_column) == (line, column) {
            return Ok(byte);
        }
        match character {
            '\n' => {
                current_line += 1;
                current_column = 1;
            }
            '\r' => {}
            _ => current_column += 1,
        }
    }
    ensure!(
        (current_line, current_column) == (line, column),
        "mutant span is outside source bytes"
    );
    Ok(text.len())
}

#[cfg(test)]
#[path = "mutation_scope_tests.rs"]
mod tests;
