//! Keep process-wide configuration checks in a separate integration binary.
use hardgate::{evidence::configure_scratch_root, runtime_resources};

#[test]
fn invalid_settings_can_be_corrected_but_active_configuration_cannot_be_replaced() {
    assert!(runtime_resources::configure_workload_jobs(Some(0)).is_err());
    runtime_resources::configure_workload_jobs(Some(2)).unwrap();
    assert!(runtime_resources::configure_workload_jobs(Some(1)).is_err());
    assert!((1..=2).contains(&runtime_resources::worker_limit(None).unwrap()));

    assert!(runtime_resources::configure_memory(Some(1), Some(2048)).is_err());
    assert!(runtime_resources::configure_memory(Some(4096), Some(1)).is_err());
    runtime_resources::configure_memory(Some(4096), Some(2048)).unwrap();
    assert!(runtime_resources::configure_memory(Some(1024), Some(1024)).is_err());

    assert!(configure_scratch_root(Some(std::path::PathBuf::new())).is_err());
    configure_scratch_root(Some(std::env::temp_dir())).unwrap();
    assert!(configure_scratch_root(None).is_err());
}
