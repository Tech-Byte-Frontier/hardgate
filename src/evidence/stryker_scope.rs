//! Bind Stryker's native file inventory, including zero-mutant files, and its
//! complete mutation plan independently from the score report.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeScope {
    schema_version: u32,
    producer_version: String,
    sources: BTreeMap<PathBuf, String>,
    mutants: BTreeSet<(PathBuf, String)>,
    baseline_passed: bool,
    completed: bool,
}

pub(super) fn prepare(output: &Path) -> Result<PathBuf> {
    let path = output.join("hardgate-scope-reporter.mjs");
    let body = REPORTER.replace(
        "\"HARDGATE_SCOPE_DESTINATION\"",
        &serde_json::to_string(&output.join("mutation.scope.json"))?,
    );
    std::fs::write(&path, body)?;
    Ok(path)
}

pub(super) fn merge(content: &str, path: &Path) -> Result<Vec<u8>> {
    let scope = path.with_extension("scope.json");
    if !scope.exists() {
        return Ok(content.as_bytes().to_vec());
    }
    let mut value: serde_json::Value = serde_json::from_str(content)?;
    ensure!(
        value.get("hardgate_scope").is_none(),
        "native report already contains an unexpected Hardgate scope extension"
    );
    value["hardgate_scope"] = serde_json::from_slice(&std::fs::read(scope)?)?;
    Ok(serde_json::to_vec(&value)?)
}

pub(super) fn sources(
    value: &serde_json::Value,
    inputs: &super::Snapshot,
) -> Result<Option<BTreeSet<PathBuf>>> {
    let Some(scope) = value.get("hardgate_scope") else {
        return Ok(None);
    };
    let scope: NativeScope = serde_json::from_value(scope.clone())?;
    ensure!(
        scope.schema_version == 1 && scope.baseline_passed && scope.completed,
        "Stryker scope collection did not complete with a passing baseline"
    );
    ensure!(
        value
            .pointer("/framework/version")
            .and_then(serde_json::Value::as_str)
            == Some(scope.producer_version.as_str()),
        "Stryker scope and report producer versions differ"
    );
    let mut reported = BTreeSet::new();
    for (name, file) in value["files"]
        .as_object()
        .context("missing Stryker file records")?
    {
        ensure!(
            scope.sources.contains_key(Path::new(name)),
            "mutation report contains an unplanned source {name}"
        );
        for mutant in file["mutants"]
            .as_array()
            .context("missing mutant records")?
        {
            let id = mutant["id"].as_str().context("mutant has no native ID")?;
            ensure!(
                reported.insert((PathBuf::from(name), id.to_string())),
                "duplicate native mutant identity"
            );
        }
    }
    ensure!(
        reported == scope.mutants,
        "Stryker report omitted or added mutants relative to its native execution plan"
    );
    for (path, hash) in &scope.sources {
        ensure!(
            inputs.0.get(path) == Some(hash),
            "Stryker selected source differs from bound execution inputs: {}",
            path.display()
        );
    }
    Ok(Some(scope.sources.into_keys().collect()))
}

const REPORTER: &str = r#"
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const require = createRequire(pathToFileURL(path.resolve('package.json')));
const version = require('@stryker-mutator/core/package.json').version;
if (version.split('.')[0] !== '10') throw new Error('Exhaustive Hardgate scope integration requires StrykerJS 10');
const coreRequire = createRequire(require.resolve('@stryker-mutator/core'));
const { commonTokens, declareClassPlugin, PluginKind } = await import(pathToFileURL(coreRequire.resolve('@stryker-mutator/api/plugin')).href);
function relative(name) {
  const result = path.relative(process.cwd(), name).split(path.sep).join('/');
  if (!result || result.startsWith('../') || path.isAbsolute(result)) throw new Error('Stryker scope left the project');
  return result;
}
class ScopeReporter {
  static inject = [commonTokens.fileDescriptions];
  constructor(files) { this.files = files; this.baseline = false; this.plans = null; }
  onDryRunCompleted() { this.baseline = true; }
  onMutationTestingPlanReady(event) {
    this.plans = event.mutantPlans.map(({ mutant }) => [relative(mutant.fileName), String(mutant.id)]).sort();
  }
  onMutationTestReportReady(report) { this.task = this.save(report); }
  async save(report) {
    if (!this.baseline || !this.plans) throw new Error('Stryker did not complete baseline and plan events');
    const reported = Object.entries(report.files).flatMap(([name, file]) => file.mutants.map(mutant => [name, String(mutant.id)])).sort();
    if (JSON.stringify(reported) !== JSON.stringify(this.plans)) throw new Error('Stryker reported an incomplete mutant plan');
    const sources = {};
    for (const [name, descriptor] of Object.entries(this.files)) {
      if (descriptor.mutate === false) continue;
      if (descriptor.mutate !== true) throw new Error('Partial mutation ranges cannot establish exhaustive evidence');
      sources[relative(name)] = createHash('sha256').update(await readFile(name)).digest('hex');
    }
    await writeFile("HARDGATE_SCOPE_DESTINATION", JSON.stringify({ schema_version: 1, producer_version: version, sources, mutants: this.plans, baseline_passed: true, completed: true }));
  }
  async wrapUp() { await this.task; }
}
export const strykerPlugins = [declareClassPlugin(PluginKind.Reporter, 'hardgate-scope', ScopeReporter)];
"#;

#[cfg(test)]
#[path = "stryker_scope_tests.rs"]
mod tests;
