use super::{EvidenceOptions, Producer};
use anyhow::{Result, bail, ensure};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct CommandSpec {
    pub tokens: Vec<String>,
    pub prerequisite: Option<Vec<String>>,
    pub version: Vec<String>,
    pub report: PathBuf,
}

pub(super) fn prepare(options: &EvidenceOptions, root: &Path) -> Result<CommandSpec> {
    let output = root.join(super::snapshot::OUTPUT_DIRECTORY).join("run");
    fs::create_dir_all(&output)?;
    match options.producer {
        Producer::CargoLlvmCov => rust_coverage(options, output),
        Producer::CargoMutants => rust_mutation(options, root, output),
        Producer::Vitest => javascript_coverage(options, root, output),
        Producer::Stryker => stryker(options, root, output),
    }
}

fn cargo_prefix(options: &EvidenceOptions) -> Vec<String> {
    let mut tokens = vec!["cargo".into()];
    if let Some(toolchain) = &options.toolchain {
        tokens.push(format!("+{toolchain}"));
    }
    tokens
}

fn rust_coverage(options: &EvidenceOptions, output: PathBuf) -> Result<CommandSpec> {
    ensure!(
        options.toolchain.is_some(),
        "branch and doctest coverage requires --toolchain <installed-nightly>; see scripts/coverage.sh for this repository's pin"
    );
    let mut tokens = cargo_prefix(options);
    tokens.push("llvm-cov".into());
    let mut version = tokens.clone();
    version.push("--version".into());
    let report = output.join("coverage.lcov");
    if !options
        .args
        .iter()
        .any(|arg| arg.split('=').next() == Some("--package"))
    {
        tokens.push("--workspace".into());
    }
    tokens.extend(strings(&["--locked", "--branch", "--include-build-script"]));
    tokens.extend(rust_scope(&options.args, false)?);
    let prerequisite = if has_targets(&options.args) {
        None
    } else {
        let mut first = tokens
            .iter()
            .filter(|token| token.as_str() != "--include-build-script")
            .cloned()
            .collect::<Vec<_>>();
        first.extend(strings(&["--all-targets", "--no-report"]));
        tokens.extend(strings(&["--doc", "--no-clean"]));
        Some(first)
    };
    tokens.extend(strings(&["--lcov", "--output-path"]));
    tokens.push(report.display().to_string());
    Ok(CommandSpec {
        tokens,
        prerequisite,
        version,
        report,
    })
}

fn rust_mutation(options: &EvidenceOptions, root: &Path, output: PathBuf) -> Result<CommandSpec> {
    validate_mutation_config(root)?;
    let mut tokens = cargo_prefix(options);
    tokens.push("mutants".into());
    let mut version = tokens.clone();
    version.push("--version".into());
    // In-place is confined to Hardgate's independent copy. Verify restoration
    // against its pre-run input snapshot before issuing a receipt.
    if !options
        .args
        .iter()
        .any(|arg| arg.split('=').next() == Some("--package"))
    {
        tokens.push("--workspace".into());
    }
    tokens.extend(strings(&[
        "--test-workspace=true",
        "--baseline=run",
        "--in-place",
        "--cargo-arg=--locked",
        "--output",
    ]));
    tokens.push(output.display().to_string());
    tokens.extend(rust_scope(&options.args, true)?);
    Ok(CommandSpec {
        tokens,
        prerequisite: Some(mutation_baseline(options, root)?),
        version,
        report: output.join("mutants.out/outcomes.json"),
    })
}

fn mutation_baseline(options: &EvidenceOptions, root: &Path) -> Result<Vec<String>> {
    // cargo-mutants 27 runs its baseline only for packages containing selected
    // mutants, even with --test-workspace=true. Prove the full test scope on
    // original code so an existing failure elsewhere cannot become a kill.
    let mut tokens = cargo_prefix(options);
    tokens.extend(strings(&["test", "--workspace", "--locked"]));
    let selected = rust_scope(&options.args, true)?;
    let mut position = 0;
    while position < selected.len() {
        let key = selected[position].as_str();
        let filtered = matches!(
            key,
            "--package" | "--re" | "--file" | "--shard" | "--timeout" | "--build-timeout"
        );
        if filtered {
            position += 2;
        } else {
            let argument = key
                .strip_prefix("--cargo-arg=")
                .or_else(|| key.strip_prefix("--cargo-test-arg="))
                .unwrap_or(key);
            tokens.push(argument.to_string());
            position += 1;
        }
    }
    let path = root.join(".cargo/mutants.toml");
    if path.exists() {
        let config: toml::Value = toml::from_str(&fs::read_to_string(path)?)?;
        append_baseline_settings(&mut tokens, &config);
        for key in ["additional_cargo_args", "additional_cargo_test_args"] {
            if let Some(args) = config.get(key).and_then(toml::Value::as_array) {
                tokens.extend(
                    args.iter()
                        .filter_map(toml::Value::as_str)
                        .map(str::to_owned),
                );
            }
        }
    }
    Ok(tokens)
}

fn append_baseline_settings(tokens: &mut Vec<String>, config: &toml::Value) {
    for (key, flag) in [
        ("all_features", "--all-features"),
        ("no_default_features", "--no-default-features"),
    ] {
        if config.get(key).and_then(toml::Value::as_bool) == Some(true)
            && !tokens.iter().any(|token| token == flag)
        {
            tokens.push(flag.into());
        }
    }
    if let Some(features) = config.get("features").and_then(toml::Value::as_array) {
        tokens.extend(
            features
                .iter()
                .filter_map(toml::Value::as_str)
                .map(|value| format!("--features={value}")),
        );
    }
    if let Some(profile) = config.get("profile").and_then(toml::Value::as_str) {
        tokens.push(format!("--profile={profile}"));
    }
}

fn has_targets(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.split('=').next(),
            Some(
                "--lib"
                    | "--bin"
                    | "--bins"
                    | "--test"
                    | "--tests"
                    | "--example"
                    | "--examples"
                    | "--bench"
                    | "--benches"
                    | "--all-targets"
            )
        )
    })
}

fn rust_scope(args: &[String], mutation: bool) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut position = 0;
    while position < args.len() {
        let (key, inline) = args[position]
            .split_once('=')
            .map_or((args[position].as_str(), None), |(key, value)| {
                (key, Some(value))
            });
        let flag = matches!(
            key,
            "--all-features"
                | "--no-default-features"
                | "--lib"
                | "--bins"
                | "--tests"
                | "--examples"
                | "--benches"
                | "--all-targets"
                | "--offline"
        );
        let value_flag = matches!(
            key,
            "--package" | "--features" | "--target" | "--bin" | "--test" | "--example" | "--bench"
        ) || (mutation
            && matches!(
                key,
                "--re" | "--file" | "--shard" | "--timeout" | "--build-timeout"
            ));
        ensure!(
            flag || value_flag,
            "unsupported producer option `{key}`; only explicit package/target/feature scope and mutation filters/timeouts are accepted"
        );
        let value = if value_flag {
            Some(scope_value(args, &mut position, key, inline)?)
        } else {
            ensure!(inline.is_none(), "{key} takes no value");
            None
        };
        append_scope_option(&mut result, key, value, mutation);
        position += 1;
    }
    Ok(result)
}

fn append_scope_option(result: &mut Vec<String>, key: &str, value: Option<&str>, mutation: bool) {
    let target = is_target_option(key);
    if mutation && (target || key == "--target" || key == "--offline") {
        let pass = if target {
            "--cargo-test-arg"
        } else {
            "--cargo-arg"
        };
        result.push(format!("{pass}={key}"));
        if let Some(value) = value {
            result.push(format!("{pass}={value}"));
        }
    } else {
        result.push(key.into());
        if let Some(value) = value {
            result.push(value.into());
        }
    }
}

fn scope_value<'a>(
    args: &'a [String],
    position: &mut usize,
    key: &str,
    inline: Option<&'a str>,
) -> Result<&'a str> {
    let value = if let Some(value) = inline {
        value
    } else {
        *position += 1;
        args.get(*position)
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing value for {key}"))?
    };
    ensure!(
        !value.is_empty() && !value.starts_with('-'),
        "invalid value for {key}"
    );
    Ok(value)
}

fn is_target_option(key: &str) -> bool {
    matches!(
        key,
        "--lib"
            | "--bin"
            | "--bins"
            | "--test"
            | "--tests"
            | "--example"
            | "--examples"
            | "--bench"
            | "--benches"
            | "--all-targets"
    )
}

fn validate_mutation_config(root: &Path) -> Result<()> {
    let path = root.join(".cargo/mutants.toml");
    if !path.exists() {
        return Ok(());
    }
    let config: toml::Value = toml::from_str(&fs::read_to_string(path)?)?;
    ensure!(
        config
            .get("test_tool")
            .is_none_or(|value| value.as_str() == Some("cargo")),
        "cargo-mutants evidence currently requires test_tool = cargo; other test tools need a matching verified workspace baseline integration"
    );
    for key in ["additional_cargo_args", "additional_cargo_test_args"] {
        if let Some(args) = config.get(key) {
            let args = args
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("{key} must be an array"))?;
            for arg in args {
                let value = arg
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("{key} must contain strings"))?;
                ensure!(
                    !matches!(
                        value.split('=').next(),
                        Some(
                            "--no-run"
                                | "--list"
                                | "--help"
                                | "--version"
                                | "--manifest-path"
                                | "--target-dir"
                                | "--config"
                        )
                    ),
                    "mutation config `{key}` contains a non-executing or redirected scope option `{value}`"
                );
            }
        }
    }
    Ok(())
}

fn javascript_coverage(
    options: &EvidenceOptions,
    root: &Path,
    output: PathBuf,
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
    ensure!(
        options.args.is_empty(),
        "configure JS package/workspace test scope in the project's test-runner configuration; arbitrary producer flags are not accepted"
    );
    Ok(CommandSpec {
        tokens,
        prerequisite: None,
        version,
        report: output.join("lcov.info"),
    })
}

fn stryker(options: &EvidenceOptions, root: &Path, output: PathBuf) -> Result<CommandSpec> {
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
    let [config] = configs.as_slice() else {
        bail!(
            "Stryker requires exactly one supported project config; detected {}",
            configs.len()
        );
    };
    let config_path = root.join(config);
    let load = if config.ends_with(".json") {
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
    let body = format!(
        "import {{ readFileSync }} from 'node:fs';\nimport {{ pathToFileURL }} from 'node:url';\nconst project = {load};\nexport default {{ ...project, reporters: ['json'], jsonReporter: {{ fileName: {} }}, incremental: false, force: true, dryRunOnly: false, allowEmpty: false, inPlace: false, concurrency: 1, fileLogLevel: 'off', tempDirName: {}, cleanTempDir: true }};\n",
        serde_json::to_string(&report)?,
        serde_json::to_string(&output.join("sandbox"))?
    );
    fs::write(&derived, body)?;
    Ok(CommandSpec {
        prerequisite: None,
        version: vec![executable.clone(), "--version".into()],
        tokens: vec![executable, "run".into(), derived.display().to_string()],
        report,
    })
}

fn local_tool(root: &Path, tool: &str) -> Result<String> {
    let path = root.join("node_modules/.bin").join(tool);
    ensure!(
        path.is_file(),
        "missing repository-local `{tool}`; install declared producer dependencies before generating evidence"
    );
    Ok(path.display().to_string())
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
