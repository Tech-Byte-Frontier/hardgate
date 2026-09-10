use super::*;

#[test]
fn rss_diagnostics_tolerate_exited_processes_and_preserve_unknown_samples() {
    let root = crate::fs_tests::tempdir("memory-diagnostics");
    assert!(counter(&root, "memory.current").is_none());
    assert!(workload_rss(&root).is_none());
    std::fs::write(root.join("memory.current"), "123\n").unwrap();
    assert_eq!(counter(&root, "memory.current"), Some(123));
    std::fs::write(root.join("memory.current"), "max\n").unwrap();
    assert!(counter(&root, "memory.current").is_none());
    std::fs::write(
        root.join("cgroup.procs"),
        format!("4294967295\n{}\n", std::process::id()),
    )
    .unwrap();
    assert!(workload_rss(&root).is_some_and(|bytes| bytes > 0));
    std::fs::write(root.join("cgroup.procs"), "invalid-pid\n").unwrap();
    assert!(workload_rss(&root).is_none());
    std::fs::remove_dir_all(root).unwrap();
}
