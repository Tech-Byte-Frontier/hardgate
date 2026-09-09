use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_MEMORY: u64 = 4 * 1024 * 1024 * 1024;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hardgate-runtime-limits-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        for (name, value) in [
            ("cpu.max", "200000 100000"),
            ("memory.max", "134217728"),
            ("memory.high", "100663296"),
            ("memory.swap.max", "0"),
            ("pids.max", "64"),
            ("memory.events", "low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\n"),
            ("pids.events", "max 0\n"),
        ] {
            fs::write(path.join(name), value).unwrap();
        }
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn every_required_kernel_limit_is_checked_without_trusting_environment_hints() {
    let fixture = Fixture::new();
    assert!(bounded(&fixture.0, MAX_MEMORY, 2).unwrap());
    for (name, value) in [
        ("cpu.max", "max 100000"),
        ("cpu.max", "200001 100000"),
        ("memory.max", "max"),
        ("memory.max", "0"),
        ("memory.max", "18446744073709551615"),
        ("memory.high", "max"),
        ("memory.high", "0"),
        ("memory.high", "134217728"),
        ("memory.swap.max", "1"),
        ("pids.max", "max"),
        ("pids.max", "0"),
        ("pids.max", "257"),
    ] {
        let path = fixture.0.join(name);
        let old = fs::read(&path).unwrap();
        fs::write(&path, value).unwrap();
        assert!(
            !bounded(&fixture.0, MAX_MEMORY, 2).unwrap(),
            "{name}={value}"
        );
        fs::write(path, old).unwrap();
    }
    fs::remove_file(fixture.0.join("cpu.max")).unwrap();
    assert!(!bounded(&fixture.0, MAX_MEMORY, 2).unwrap());
}

#[test]
fn kernel_limit_and_event_changes_invalidate_completed_work() {
    let fixture = Fixture::new();
    let path = fixture.0.join("memory.events");
    let boundary = Boundary {
        directory: fixture.0.clone(),
        memory_limit: MAX_MEMORY,
        jobs: 2,
        events: vec![(path.clone(), event_snapshot(&path).unwrap())],
    };
    boundary.verify().unwrap();
    fs::write(&path, "low 0\nhigh 1\nmax 0\noom 0\noom_kill 0\n").unwrap();
    boundary.verify().unwrap();
    fs::write(&path, "low 0\nhigh 1\nmax 1\noom 0\noom_kill 0\n").unwrap();
    assert!(
        boundary
            .verify()
            .unwrap_err()
            .to_string()
            .contains("limit events")
    );
    fs::write(fixture.0.join("memory.swap.max"), "max").unwrap();
    assert!(
        boundary
            .verify()
            .unwrap_err()
            .to_string()
            .contains("limits changed")
    );
}

#[test]
fn cpu_quota_parsing_rejects_invalid_zero_and_overflow_values() {
    for value in [
        "",
        "max 100000",
        "0 100000",
        "1 0",
        "1",
        "1 2 3",
        "-1 100000",
        "200001 100000",
    ] {
        assert!(!cpu_bounded(value, 2), "{value}");
    }
    assert!(cpu_bounded("50000 100000\n", 2));
    assert!(cpu_bounded("18446744073709551615 18446744073709551615", 2));
}

#[test]
fn larger_allowances_still_reject_unbounded_or_excessive_kernel_limits() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("cpu.max"), "800000 100000").unwrap();
    fs::write(fixture.0.join("pids.max"), "1024").unwrap();
    assert!(bounded(&fixture.0, MAX_MEMORY, 8).unwrap());
    assert!(!bounded(&fixture.0, MAX_MEMORY, 2).unwrap());
    fs::write(fixture.0.join("pids.max"), "1025").unwrap();
    assert!(!bounded(&fixture.0, MAX_MEMORY, 8).unwrap());
    fs::write(fixture.0.join("pids.max"), "1024").unwrap();
    fs::write(fixture.0.join("cpu.max"), "800001 100000").unwrap();
    assert!(!bounded(&fixture.0, MAX_MEMORY, 8).unwrap());
}

#[test]
fn task_failure_reports_the_limit_and_usage_without_claiming_memory_exhaustion() {
    let fixture = Fixture::new();
    for (name, value) in [("pids.current", "63"), ("pids.peak", "64")] {
        fs::write(fixture.0.join(name), value).unwrap();
    }
    let message = event_failure(
        &fixture.0,
        &fixture.0.join("pids.events"),
        "max 0\n",
        "max 7\n",
    );
    assert!(message.contains("task limit events"));
    assert!(message.contains("pids.current=63, pids.peak=64, pids.max=64"));
    assert!(message.contains("counters [max 0] -> [max 7]"));
    assert!(!message.contains("memory limit events"));
}

#[test]
fn event_evidence_requires_valid_unique_allocation_counters() {
    let fixture = Fixture::new();
    let path = fixture.0.join("memory.events");
    for invalid in [
        "max 0\n",
        "max 0\noom bad\noom_kill 0\n",
        "max 0\nmax 0\noom 0\noom_kill 0\n",
        "max 0 extra\n",
    ] {
        fs::write(&path, invalid).unwrap();
        assert!(event_snapshot(&path).is_err());
    }
}

#[test]
fn unreadable_and_oversized_kernel_evidence_is_an_error() {
    let fixture = Fixture::new();
    let path = fixture.0.join("cpu.max");
    fs::write(&path, "0".repeat(8193)).unwrap();
    assert!(
        bounded(&fixture.0, MAX_MEMORY, 2)
            .unwrap_err()
            .to_string()
            .contains("oversized")
    );
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(bounded(&fixture.0, MAX_MEMORY, 2).is_err());
    let limit = fixture.0.join("memory.max");
    fs::write(&limit, "invalid").unwrap();
    assert!(
        numeric_limit(&fixture.0, "memory.max")
            .unwrap_err()
            .to_string()
            .contains("invalid kernel limit")
    );
    fs::remove_file(&limit).unwrap();
    assert_eq!(numeric_limit(&fixture.0, "memory.max").unwrap(), None);
}
