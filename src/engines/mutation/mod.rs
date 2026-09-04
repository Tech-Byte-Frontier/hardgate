pub mod gatekeeper;
pub mod generator;
pub mod js;
mod js_command;
pub(crate) mod js_manifest;
mod js_tests;
mod process;
pub mod runner;
pub(crate) mod target;

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP_ROOT: AtomicUsize = AtomicUsize::new(0);

    pub(crate) fn temp_root(prefix: &str, label: &str) -> PathBuf {
        let id = NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("{prefix}-{label}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
}

pub use gatekeeper::{MutationGatekeeper, MutationStats, MutationViolation};
pub use generator::{AstMutant, AstMutationGenerator};
pub use js::{PackageManager, ResolvedTestPlan, TestFramework, TestSelection};
pub use runner::{
    BaselineExecutionResult, BaselineOutcome, DEFAULT_TIMEOUT_SECS, FULL_SUITE_TIMEOUT_SECS,
    MutantExecutionResult, MutantOutcome, NativeMutationRunner,
};
