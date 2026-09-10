use super::producer_rust::{rust_coverage, rust_mutation};
use super::{EvidenceOptions, Producer};
use anyhow::{Result, bail, ensure};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct CommandSpec {
    pub tokens: Vec<String>,
    pub prerequisite: Option<Vec<String>>,
    pub auxiliary: Vec<Vec<String>>,
    pub version: Vec<String>,
    pub report: PathBuf,
}

pub(super) fn prepare(
    options: &EvidenceOptions,
    root: &Path,
    partition: Option<&super::partitions::Partition>,
) -> Result<CommandSpec> {
    let output = root.join(super::snapshot::OUTPUT_DIRECTORY).join("run");
    fs::create_dir_all(&output)?;
    match options.producer {
        Producer::CargoLlvmCov => rust_coverage(options, output),
        Producer::CargoMutants => rust_mutation(options, root, output, partition),
        Producer::Vitest => javascript_coverage(options, root, output, partition),
        Producer::Stryker => stryker(options, root, output, partition),
        Producer::Pytest => super::python::prepare(options, root, output, partition),
    }
}

fn javascript_coverage(
    options: &EvidenceOptions,
    root: &Path,
    output: PathBuf,
    partition: Option<&super::partitions::Partition>,
) -> Result<CommandSpec> {
    ensure!(
        options.toolchain.is_none(),
        "--toolchain applies only to Rust producers"
    );
    let executable = local_tool(root, "vitest")?;
    let version = vec![executable.clone(), "--version".into()];
    let mut tokens = vec![executable];
    tokens.extend(strings(&[
        "run",
        "--coverage.enabled=true",
        "--coverage.provider=v8",
        "--coverage.reporter=lcov",
    ]));
    tokens.push(format!("--coverage.reportsDirectory={}", output.display()));
    if let Some(partition) = partition {
        if let Some(path) = &partition.config.config {
            tokens.push(format!("--config={}", config_path(root, path)?.display()));
        }
        for path in &partition.sources {
            tokens.push(format!("--coverage.include={}", path.display()));
        }
    }
    ensure!(
        options.args.is_empty(),
        "configure JS package/workspace test scope in the project's test-runner configuration; arbitrary producer flags are not accepted"
    );
    Ok(CommandSpec {
        tokens,
        prerequisite: None,
        auxiliary: vec![],
        version,
        report: output.join("lcov.info"),
    })
}

fn stryker(
    options: &EvidenceOptions,
    root: &Path,
    output: PathBuf,
    partition: Option<&super::partitions::Partition>,
) -> Result<CommandSpec> {
    ensure!(
        options.toolchain.is_none() && options.args.is_empty(),
        "configure Stryker scope in stryker.config.json, .js, .cjs or .mjs"
    );
    let executable = local_tool(root, "stryker")?;
    let configs: Vec<_> = [
        "stryker.config.json",
        "stryker.config.js",
        "stryker.config.cjs",
        "stryker.config.mjs",
    ]
    .into_iter()
    .filter(|name| root.join(name).is_file())
    .collect();
    let selected = partition.and_then(|partition| partition.config.config.as_deref());
    let config = match (selected, configs.as_slice()) {
        (Some(config), _) => config,
        (None, [config]) => Path::new(config),
        _ => bail!(
            "Stryker requires exactly one supported project config or a named producer configuration; detected {}",
            configs.len()
        ),
    };
    let config_path = config_path(root, config)?;
    let load = if config
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        format!(
            "JSON.parse(readFileSync({}, 'utf8'))",
            serde_json::to_string(&config_path)?
        )
    } else {
        format!(
            "(await import(pathToFileURL({}).href)).default",
            serde_json::to_string(&config_path)?
        )
    };
    let report = output.join("mutation.json");
    let derived = output.join("stryker.config.mjs");
    let plan = crate::resources::worker_plan::mutation_workers()?;
    crate::engines::process::diagnostic(format_args!(
        "hardgate: mutation worker plan: {}; runner memory is an estimate, aggregate limits remain enforced",
        serde_json::to_string(&plan)?
    ));
    let body = format!(
        "import {{ readFileSync }} from 'node:fs';\nimport {{ pathToFileURL }} from 'node:url';\nconst project = {load};\nconst maximum = {};\nif (project.concurrency !== undefined && (!Number.isInteger(project.concurrency) || project.concurrency < 1)) throw new Error('Stryker concurrency must be a positive integer');\nconst concurrency = Math.min(project.concurrency ?? maximum, maximum);\nexport default {{ ...project, ignoreStatic: false, reporters: ['json', 'progress-append-only'], jsonReporter: {{ fileName: {} }}, incremental: false, force: true, dryRunOnly: false, allowEmpty: false, inPlace: false, concurrency, fileLogLevel: 'off', tempDirName: {}, cleanTempDir: true }};\n",
        plan.jobs,
        serde_json::to_string(&report)?,
        serde_json::to_string(&output.join("sandbox"))?
    );
    let body = if let Some(partition) = partition {
        let mut body = body;
        let sources = serde_json::to_string(&partition.sources)?;
        let plugin = serde_json::to_string(&super::stryker_scope::prepare(&output)?)?;
        body = body.replace("...project, ignoreStatic: false, reporters:", &format!("...project, mutate: {sources}, ignoreStatic: false, ignorers: [], ignorePatterns: [], mutator: {{ ...project.mutator, excludedMutations: [] }}, appendPlugins: [...(project.appendPlugins ?? []), {plugin}], reporters:"));
        body = body.replace(
            "['json', 'progress-append-only']",
            "['json', 'progress-append-only', 'hardgate-scope']",
        );
        body
    } else {
        body
    };
    fs::write(&derived, body)?;
    Ok(CommandSpec {
        prerequisite: None,
        auxiliary: vec![],
        version: vec![executable.clone(), "--version".into()],
        tokens: vec![executable, "run".into(), derived.display().to_string()],
        report,
    })
}

fn config_path(root: &Path, path: &Path) -> Result<PathBuf> {
    ensure!(
        crate::config::evidence::safe_relative(path) && !super::snapshot::omitted(path, true),
        "producer config must be a source-bound repository input"
    );
    let target = root.join(path).canonicalize()?;
    ensure!(
        target.starts_with(root) && target.is_file(),
        "producer config must resolve to a file inside the repository"
    );
    Ok(target)
}

fn local_tool(root: &Path, tool: &str) -> Result<String> {
    let path = root.join("node_modules/.bin").join(tool);
    ensure!(
        path.is_file(),
        "missing repository-local `{tool}`; install declared producer dependencies before generating evidence"
    );
    Ok(path.display().to_string())
}

pub(super) fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
