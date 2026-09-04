#[path = "../support/fs.rs"]
mod test_fs;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub struct Fixture(pub PathBuf);

impl Fixture {
    pub fn new(prefix: &str, tag: &str) -> Self {
        Self(test_fs::tempdir(&format!("{prefix}-{tag}")))
    }

    pub fn with_config(prefix: &str, tag: &str, config: &str) -> Self {
        let fixture = Self::new(prefix, tag);
        fixture.write("hardgate.toml", config);
        fixture
    }

    pub fn with_files(prefix: &str, tag: &str, config: &str, files: &[(&str, &str)]) -> Self {
        let fixture = Self::with_config(prefix, tag, config);
        for &(path, content) in files {
            fixture.write(path, content);
        }
        fixture
    }

    pub fn write(&self, path: &str, content: &str) {
        let target = self.0.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, content).unwrap();
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

pub fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("hardgate binary should run")
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stdout={} stderr={}",
        stdout(output),
        stderr(output)
    );
}

pub fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}: {}; stderr: {}",
            stdout(output),
            stderr(output)
        )
    })
}
