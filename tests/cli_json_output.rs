#[path = "support/fs.rs"]
mod fs;

use fs::tempdir;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BASE_CONFIG: &str = r#"[gate]
preset = "custom"
strict = true
enforce_classified_sources = true

[budgets.files]
max_bytes = 100000

[budgets.files.max_lines]
default = 10000
rs = 10000

[budgets.functions]
max_cyclomatic = 100
max_parameters = 20
max_lines = 1000
max_nesting_depth = 20

[anti_gaming]
disallow_suppressions = true

[coverage]
enabled = false

[mutation]
enabled = false
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, config: &str, source: Option<(&str, &str)>) -> Self {
        let root = tempdir(&format!("cli-json-{tag}"));
        write(&root, "hardgate.toml", config);
        if let Some((path, content)) = source {
            write(&root, path, content);
        }
        Self(root)
    }
}

impl AsRef<Path> for Fixture {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    target
        .parent()
        .map(std::fs::create_dir_all)
        .transpose()
        .unwrap();
    std::fs::write(target, content).unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("hardgate binary should run")
}

fn parse_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout must contain exactly one JSON document: {error}: {stdout}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn policy_and_scan_json_have_no_progress_prefix_or_suffix() {
    let fixture = Fixture::new(
        "static",
        BASE_CONFIG,
        Some(("src/lib.rs", "pub fn answer() -> i32 { 42 }\n")),
    );
    for args in [
        &["check", "--checks", "policy", "--json"][..],
        &["scan", "src/lib.rs", "--format", "json"][..],
    ] {
        let output = run(fixture.as_ref(), args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = parse_stdout(&output);
        assert_eq!(report["passed"], true);
    }
}
