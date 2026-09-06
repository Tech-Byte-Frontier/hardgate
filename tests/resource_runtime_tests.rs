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
detached=$(setsid sh -c 'cut -d: -f3 /proc/self/cgroup')
test "$cg" = "$detached"
set -- $(cat "/sys/fs/cgroup$cg/cpu.max" "/sys/fs/cgroup$cg/memory.max" "/sys/fs/cgroup$cg/memory.high" "/sys/fs/cgroup$cg/memory.swap.max" "/sys/fs/cgroup$cg/pids.max")
test "$1" -gt 0
test "$1" -le "$((2 * $2))"
test "$3" -le 4294967296
test "$4" -le "$(($3 / 5 * 4))"
test "$5" -eq 0
test "$6" -le 256
test "$CARGO_BUILD_JOBS" -ge 1
test "$CARGO_BUILD_JOBS" -le 2
test "$RAYON_NUM_THREADS" -ge 1
test "$RAYON_NUM_THREADS" -le 2
"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .current_dir(&fixture.0)
        .args(["check", "--checks", "tests", "--json"])
        .env("CARGO_BUILD_JOBS", "64")
        .env("RAYON_NUM_THREADS", "64")
        .output()
        .unwrap();
    assert_status(&output, true, "contained orchestration");
    assert_eq!(json(&output)["passed"], true);
    let report = json(&output);
    assert!(
        report["execution"]["engines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|engine| engine["id"] == "tests" && engine["state"] == "completed")
    );
}

#[test]
fn excessive_workers_never_start_the_project_command() {
    let fixture = Fixture::new("resource-runtime", "reject-workers", Some(CONFIG));
    fixture.write("probe.sh", "touch started\n");
    let output = run(
        &fixture,
        &["check", "--checks", "tests", "--json", "--threads", "64"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["status"], "error");
    assert!(!fixture.join("started").exists());
}
