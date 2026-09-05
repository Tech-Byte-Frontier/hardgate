use hardgate::commands::init::InitOptions;
use hardgate::config::HardgateConfig;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static CURRENT_DIRECTORY: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) struct WorkingDirectory {
    original: PathBuf,
}

impl WorkingDirectory {
    pub(crate) fn enter(root: &Path) -> Self {
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(root).unwrap();
        Self { original }
    }
}

impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.original).unwrap();
    }
}

pub(crate) fn with_root(tag: &str, callback: impl FnOnce(&Path)) {
    let _lock = CURRENT_DIRECTORY
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "hardgate-init-experience-{}-{stamp}-{tag}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let directory = WorkingDirectory::enter(&root);
    callback(&root);
    drop(directory);
    fs::remove_dir_all(root).unwrap();
}

pub(crate) fn options(preset: &str) -> InitOptions {
    InitOptions {
        preset: preset.to_string(),
        ..InitOptions::default()
    }
}

pub(crate) fn load_written(root: &Path) -> HardgateConfig {
    HardgateConfig::load_or_default(Some(&root.join("hardgate.toml"))).unwrap()
}

pub(crate) fn assert_commands(config: &HardgateConfig, expected: [&str; 4]) {
    assert_eq!(
        config.orchestration.format_check.as_deref(),
        Some(expected[0])
    );
    assert_eq!(config.orchestration.format.as_deref(), Some(expected[1]));
    assert_eq!(config.orchestration.lint.as_deref(), Some(expected[2]));
    assert_eq!(config.orchestration.test_cmd.as_deref(), Some(expected[3]));
}
