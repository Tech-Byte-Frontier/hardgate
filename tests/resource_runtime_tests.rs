#![cfg(target_os = "linux")]

#[path = "common/cli.rs"]
mod cli;
use cli::{Fixture, assert_status, json, run};
use std::process::Command;

const CONFIG: &str = "[gate]\npreset = 'custom'\n[orchestration]\nrequire_isolation = true\ntest_cmd = 'sh probe.sh'\n";

#[test]
fn empty_inherited_scratch_root_is_rejected_before_starting_tools() {
    let fixture = Fixture::new("resource-runtime", "empty-scratch", Some(CONFIG));
    fixture.write("probe.sh", "touch started\n");
    let output = Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .current_dir(&fixture.0)
        .env("HARDGATE_SCRATCH_ROOT", "")
        .args(["check", "--checks", "tests", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).contains("scratch root must not be empty"));
    assert!(!fixture.join("started").exists());
}

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
test "$1" -le "$((HARDGATE_WORKLOAD_JOBS * $2))"
test "$3" -le 17179869184
test "$4" -le "$(($3 / 5 * 4))"
test "$5" -eq 0
task_limit=$((HARDGATE_WORKLOAD_JOBS * 128))
if [ "$task_limit" -lt 256 ]; then task_limit=256; fi
test "$6" -le "$task_limit"
test "$CARGO_BUILD_JOBS" -ge 1
test "$CARGO_BUILD_JOBS" -le "$HARDGATE_WORKLOAD_JOBS"
test "$RAYON_NUM_THREADS" -ge 1
test "$RAYON_NUM_THREADS" -le "$HARDGATE_WORKLOAD_JOBS"
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
        &["check", "--checks", "tests", "--json", "--threads", "65"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["status"], "error");
    assert!(!fixture.join("started").exists());
}

#[test]
fn invalid_workload_allowances_fail_before_starting_tools() {
    let fixture = Fixture::new("resource-runtime", "reject-workload-jobs", Some(CONFIG));
    fixture.write("probe.sh", "touch started\n");
    for count in ["0", "65", "invalid"] {
        let output = run(
            &fixture,
            &[
                "check",
                "--checks",
                "tests",
                "--json",
                "--workload-jobs",
                count,
            ],
        );
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(json(&output)["status"], "error");
        assert!(!fixture.join("started").exists());
    }
}

#[test]
fn explicit_workload_allowance_overrides_invalid_environment() {
    let fixture = Fixture::new("resource-runtime", "workload-precedence", Some(CONFIG));
    let output = Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .current_dir(&fixture.0)
        .args([
            "--workload-jobs",
            "4",
            "check",
            "--checks",
            "policy",
            "--json",
        ])
        .env("HARDGATE_WORKLOAD_JOBS", "invalid")
        .output()
        .unwrap();
    assert_status(&output, true, "explicit workload allowance");
}

#[test]
fn workload_diagnostics_preserve_jsonl_progress() {
    let fixture = Fixture::new("resource-runtime", "progress", Some(CONFIG));
    fixture.write("probe.sh", "printf 'completed\\n'\n");
    let output = run(
        &fixture,
        &[
            "check",
            "--checks",
            "tests",
            "--json",
            "--progress",
            "jsonl",
        ],
    );
    assert_status(&output, true, "JSONL workload progress");
    let events = String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("JSONL event"))
        .collect::<Vec<_>>();
    assert!(events.iter().any(|event| {
        event["stage"] == "workload_start"
            && event["message"]
                .as_str()
                .unwrap_or("")
                .contains("pids.max=")
    }));
}

#[test]
fn invalid_memory_and_worker_estimates_never_start_project_tools() {
    let fixture = Fixture::new("resource-runtime", "reject-memory", Some(CONFIG));
    fixture.write("probe.sh", "touch started\n");
    for (flag, value) in [
        ("--workload-memory-mib", "0"),
        ("--workload-memory-mib", "65537"),
        ("--mutation-worker-memory-mib", "1023"),
        ("--mutation-worker-memory-mib", "16385"),
    ] {
        let output = run(
            &fixture,
            &["check", "--checks", "tests", "--json", flag, value],
        );
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(json(&output)["status"], "error");
        assert!(!fixture.join("started").exists());
    }
}
