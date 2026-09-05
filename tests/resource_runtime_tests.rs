#![cfg(target_os = "linux")]

#[path = "common/cli.rs"]
mod cli;
use cli::{Fixture, assert_status, json, run};
use std::process::Command;

const CONFIG: &str = "[gate]\npreset = 'custom'\n[orchestration]\ntest_cmd = 'sh probe.sh'\n";

#[test]
fn orchestration_and_detached_descendants_share_enforced_kernel_limits() {
    let fixture = Fixture::new("resource-runtime", "inherit", Some(CONFIG));
    fixture.write("src/lib.rs", "pub fn answer() -> u32 { 42 }\n");
    fixture.write(
        "probe.sh",
        r#"set -eu
cg=$(cut -d: -f3 /proc/self/cgroup)
printf '%s\n' "$cg" > parent-cgroup.txt
for name in cpu.max memory.max memory.high memory.swap.max pids.max; do
  cat "/sys/fs/cgroup$cg/$name"
done > limits.txt
setsid sh -c 'cut -d: -f3 /proc/self/cgroup' > detached-cgroup.txt
printf '%s %s\n' "$CARGO_BUILD_JOBS" "$RAYON_NUM_THREADS" > workers.txt
"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .current_dir(&fixture.0)
        .args(["check", "--all", "--json"])
        .env("CARGO_BUILD_JOBS", "64")
        .env("RAYON_NUM_THREADS", "64")
        .output()
        .unwrap();
    assert_status(&output, true, "contained orchestration");
    assert_eq!(json(&output)["passed"], true);
    assert_eq!(
        std::fs::read(fixture.join("parent-cgroup.txt")).unwrap(),
        std::fs::read(fixture.join("detached-cgroup.txt")).unwrap()
    );
    let limits = std::fs::read_to_string(fixture.join("limits.txt")).unwrap();
    let values = limits
        .split_whitespace()
        .map(|value| value.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert!(values[0] > 0 && values[0] <= 2 * values[1]);
    assert!(values[2] <= 4 * 1024 * 1024 * 1024);
    assert!(values[3] <= values[2] / 5 * 4);
    assert_eq!(values[4], 0);
    assert!(values[5] <= 256);
    let workers = std::fs::read_to_string(fixture.join("workers.txt")).unwrap();
    assert!(
        workers
            .split_whitespace()
            .all(|value| (1..=2).contains(&value.parse::<u32>().unwrap()))
    );
}

#[test]
fn excessive_workers_never_start_the_project_command() {
    let fixture = Fixture::new("resource-runtime", "reject-workers", Some(CONFIG));
    fixture.write("probe.sh", "touch started\n");
    let output = run(&fixture, &["check", "--all", "--json", "--threads", "64"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["status"], "error");
    assert!(!fixture.join("started").exists());
}
