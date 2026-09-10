use super::{EvidenceOptions, partitions::Partition, producer::CommandSpec};
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

/// Run the installed Python coverage producer; all test execution stays in
/// the same independent, restored workspace as other evidence producers.
pub(super) fn prepare(
    options: &EvidenceOptions,
    root: &Path,
    output: PathBuf,
    partition: Option<&Partition>,
) -> Result<CommandSpec> {
    ensure!(
        options.toolchain.is_none(),
        "--toolchain applies only to Rust producers"
    );
    let interpreter = [".venv/bin/python", "venv/bin/python"]
        .into_iter()
        .map(|path| root.join(path))
        .find(|path| path.is_file())
        .map_or_else(|| "python3".to_string(), |path| path.display().to_string());
    let config = coverage_config(root, &output, partition)?;
    let data = output.join("python.coverage");
    let mut tests = vec![
        interpreter.clone(),
        "-m".into(),
        "coverage".into(),
        "run".into(),
        format!("--rcfile={}", config.display()),
        format!("--data-file={}", data.display()),
        "--branch".into(),
        "-m".into(),
        "pytest".into(),
    ];
    if let Some(path) = partition.and_then(|partition| partition.config.config.as_ref()) {
        ensure!(
            crate::config::evidence::safe_relative(path) && !super::snapshot::omitted(path, true),
            "pytest config must be a source-bound repository input"
        );
        ensure!(root.join(path).is_file(), "pytest config file is missing");
        tests.extend(["-c".into(), path.display().to_string()]);
    }
    for path in &options.args {
        ensure!(
            crate::config::evidence::safe_relative(Path::new(path))
                && !path.starts_with('-')
                && !path.contains("::"),
            "pytest producer arguments must be test file/directory paths; configure test options in the named pytest config"
        );
        tests.push(path.clone());
    }
    let report = output.join("coverage.lcov");
    let render = |kind: &str, destination: &Path| {
        vec![
            interpreter.clone(),
            "-m".into(),
            "coverage".into(),
            kind.into(),
            format!("--rcfile={}", config.display()),
            format!("--data-file={}", data.display()),
            "-o".into(),
            destination.display().to_string(),
        ]
    };
    Ok(CommandSpec {
        tokens: render("lcov", &report),
        auxiliary: vec![render("json", &report.with_extension("native.json"))],
        prerequisite: Some(tests),
        version: vec![
            interpreter,
            "-m".into(),
            "coverage".into(),
            "--version".into(),
        ],
        report,
    })
}

fn coverage_config(root: &Path, output: &Path, partition: Option<&Partition>) -> Result<PathBuf> {
    let sources = match partition {
        Some(partition) => partition.sources.clone(),
        None => super::partitions::source_inventory(root, &Default::default())?
            .into_iter()
            .filter(|path| path.extension().is_some_and(|extension| extension == "py"))
            .collect(),
    };
    ensure!(
        !sources.is_empty(),
        "pytest evidence requires Python source inputs"
    );
    ensure!(
        sources
            .iter()
            .all(|path| path.extension().is_some_and(|extension| extension == "py")),
        "pytest partition contains non-Python executable source"
    );
    let parents: std::collections::BTreeSet<_> = sources
        .iter()
        .map(|path| {
            root.join(path)
                .parent()
                .expect("absolute source parent")
                .to_path_buf()
        })
        .collect();
    let config = output.join("coverage.ini");
    let directories = parents
        .iter()
        .map(|path| format!("    {}\n", path.display()))
        .collect::<String>();
    let include = sources
        .iter()
        .map(|path| format!("    {}\n", root.join(path).display()))
        .collect::<String>();
    std::fs::write(
        &config,
        format!(
            "[run]\nbranch = true\nsource =\n{directories}[report]\ninclude =\n{include}exclude_lines =\nexclude_also =\npartial_branches =\n"
        ),
    )?;
    Ok(config)
}

#[cfg(test)]
#[path = "python_tests.rs"]
mod tests;
