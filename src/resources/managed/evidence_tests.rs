use super::*;
use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const MEMORY: u64 = 1024 * 1024;
const HIGH: u64 = 786_432;
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

fn fixture() -> CommandEvidence {
    CommandEvidence::create(MEMORY).expect("fixture evidence")
}

fn report(evidence: &CommandEvidence, status: u64, peak: u64, extra: &str) {
    let mut content = format!(
        "id={}\nlimit={}\nhigh={}\nstatus={status}\npeak={peak}\n\
         events.high=0\nevents.max=0\nevents.oom=0\nevents.oom_kill=0\n",
        evidence.identity(),
        evidence.memory_bytes,
        evidence.high_bytes,
    );
    content.push_str("pids_max_events=0\n");
    content.push_str(extra);
    fs::write(evidence.report_path(), content).expect("write evidence report");
}

fn exited(code: i32) -> std::process::ExitStatus {
    std::process::ExitStatus::from_raw(code << 8)
}

fn assert_error<T>(result: std::io::Result<T>) {
    assert!(result.is_err(), "expected evidence failure");
}

fn live_fixture(peak: &str, events: &str) -> PathBuf {
    let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "hardgate-evidence-cgroup-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("create live fixture");
    fs::write(directory.join("memory.peak"), peak).expect("write peak");
    fs::write(directory.join("memory.events"), events).expect("write events");
    fs::write(directory.join("pids.events"), "max 0\n").expect("write pids events");
    directory
}

fn standard_events() -> &'static str {
    "high 0\nmax 0\noom 0\noom_kill 0\n"
}

#[test]
fn create_uses_private_unique_paths_and_drop_removes_only_directory() {
    let first = fixture();
    let second = fixture();
    assert_ne!(first.identity(), second.identity());
    assert!(first.shim_path().starts_with(std::env::temp_dir()));
    assert!(first.report_path().starts_with(std::env::temp_dir()));
    let directory = first.shim_path().parent().unwrap().to_path_buf();
    let directory_metadata = fs::symlink_metadata(&directory).unwrap();
    assert_eq!(directory_metadata.mode() & 0o7777, 0o700);
    let metadata = fs::symlink_metadata(first.shim_path()).unwrap();
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    drop(first);
    assert!(!directory.exists());
    drop(second);
}

#[test]
fn shim_is_valid_posix_shell() {
    let evidence = fixture();
    let status = Command::new("/bin/sh")
        .args(["-n"])
        .arg(evidence.shim_path())
        .status()
        .expect("check shim syntax");
    assert!(status.success());
}

#[test]
fn allow_start_creates_exact_owned_evidence() {
    let evidence = fixture();
    evidence.allow_start().unwrap();
    let marker = evidence.report_path().with_extension("start");
    let metadata = fs::symlink_metadata(&marker).unwrap();
    assert!(!metadata.file_type().is_symlink());
    assert!(metadata.file_type().is_file());
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.uid(), rustix::process::getuid().as_raw());
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(
        fs::read_to_string(marker).unwrap(),
        format!("{}\n", evidence.identity())
    );
}

#[test]
fn allow_start_refuses_a_preexisting_marker() {
    let evidence = fixture();
    let marker = evidence.report_path().with_extension("start");
    fs::write(&marker, b"preexisting\n").unwrap();
    let error = evidence.allow_start().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(marker).unwrap(), "preexisting\n");
}

#[test]
fn accepts_success_and_nonzero_target_exit_evidence() {
    let success = fixture();
    report(&success, 0, 12, "");
    success.verify(&exited(0)).unwrap();

    let nonzero = fixture();
    report(&nonzero, 7, 12, "");
    nonzero.verify(&exited(7)).unwrap();
}

#[test]
fn rejects_manager_signal_and_missing_report_evidence() {
    let missing = fixture();
    assert_error(missing.verify(&exited(0)));

    let signaled = fixture();
    report(&signaled, 0, 12, "");
    assert_error(signaled.verify(&std::process::ExitStatus::from_raw(9)));
}

#[test]
fn rejects_bad_identity_status_and_duplicate_fields() {
    let bad_identity = fixture();
    report(&bad_identity, 0, 12, "");
    let content = fs::read_to_string(bad_identity.report_path())
        .unwrap()
        .replace(bad_identity.identity(), "different");
    fs::write(bad_identity.report_path(), content).unwrap();
    assert_error(bad_identity.verify(&exited(0)));

    let bad_status = fixture();
    report(&bad_status, 6, 12, "");
    assert_error(bad_status.verify(&exited(7)));

    let duplicate = fixture();
    report(&duplicate, 0, 12, "events.high=0\n");
    assert_error(duplicate.verify(&exited(0)));
}

#[test]
fn rejects_missing_required_fields_and_numeric_overflow() {
    let missing = fixture();
    fs::write(
        missing.report_path(),
        format!(
            "id={}\nlimit={}\nhigh={}\nstatus=0\npeak=1\n",
            missing.identity(),
            missing.memory_bytes,
            missing.high_bytes
        ),
    )
    .unwrap();
    assert_error(missing.verify(&exited(0)));

    let overflow = fixture();
    report(&overflow, 0, 12, "events.future=18446744073709551616\n");
    assert_error(overflow.verify(&exited(0)));
}

#[test]
fn rejects_oversized_and_symlink_reports() {
    let oversized = fixture();
    let mut content = String::from("x");
    content.push_str(&"x".repeat(MAX_REPORT_BYTES as usize));
    fs::write(oversized.report_path(), content).unwrap();
    assert_error(oversized.verify(&exited(0)));

    let symlinked = fixture();
    let target = symlinked.report_path().with_extension("target");
    fs::write(&target, "id=wrong\n").unwrap();
    symlink(&target, symlinked.report_path()).unwrap();
    assert_error(symlinked.verify(&exited(0)));
}

#[test]
fn rejects_pressure_events_even_when_status_is_zero() {
    for event in ["high", "max", "oom", "oom_kill", "oom_group_kill"] {
        let evidence = fixture();
        report(&evidence, 0, 12, "");
        let mut content = fs::read_to_string(evidence.report_path()).unwrap();
        if event == "oom_group_kill" {
            content.push_str("events.oom_group_kill=1\n");
        } else {
            content = content.replace(&format!("events.{event}=0"), &format!("events.{event}=1"));
        }
        fs::write(evidence.report_path(), content).unwrap();
        let error = evidence.verify(&exited(0)).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("counter"),
            "unexpected diagnostic: {message}"
        );
        assert!(
            message.contains("nonzero"),
            "unexpected diagnostic: {message}"
        );
    }
}

#[test]
fn rejects_nonzero_pids_events_even_when_status_is_zero() {
    let evidence = fixture();
    report(&evidence, 0, 12, "");
    let content = fs::read_to_string(evidence.report_path())
        .unwrap()
        .replace("pids_max_events=0", "pids_max_events=1");
    fs::write(evidence.report_path(), content).unwrap();
    let error = evidence.verify(&exited(0)).unwrap_err();
    assert!(error.to_string().contains("pids.events"));
    assert!(error.to_string().contains("nonzero"));
}

#[test]
fn rejects_missing_and_malformed_pids_evidence() {
    let missing = fixture();
    report(&missing, 0, 12, "");
    let content = fs::read_to_string(missing.report_path())
        .unwrap()
        .replace("pids_max_events=0\n", "");
    fs::write(missing.report_path(), content).unwrap();
    assert_error(missing.verify(&exited(0)));

    let malformed = fixture();
    report(&malformed, 0, 12, "");
    let content = fs::read_to_string(malformed.report_path())
        .unwrap()
        .replace("pids_max_events=0", "pids_max_events=bad");
    fs::write(malformed.report_path(), content).unwrap();
    assert_error(malformed.verify(&exited(0)));
}

#[test]
fn rejects_changed_memory_limits() {
    for (field, replacement) in [("limit", "99"), ("high", "79")] {
        let evidence = fixture();
        report(&evidence, 0, 12, "");
        let content = fs::read_to_string(evidence.report_path()).unwrap().replace(
            &format!("{field}={}", if field == "limit" { MEMORY } else { HIGH }),
            &format!("{field}={replacement}"),
        );
        fs::write(evidence.report_path(), content).unwrap();
        let error = evidence.verify(&exited(0)).unwrap_err();
        assert!(
            error.to_string().contains("memory limits"),
            "unexpected diagnostic: {error}"
        );
    }
}

#[test]
fn accepts_future_event_keys_without_weakening_required_checks() {
    let evidence = fixture();
    report(&evidence, 0, 12, "events.future_counter=4\n");
    evidence.verify(&exited(0)).unwrap();
}

#[test]
fn rejects_peak_at_or_above_high() {
    let evidence = fixture();
    report(&evidence, 0, HIGH, "");
    assert_error(evidence.verify(&exited(0)));
}

#[test]
fn live_check_reads_bounded_peak_and_events() {
    let directory = live_fixture("12\n", standard_events());
    check_live(&directory, HIGH).unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn live_check_rejects_pressure_and_preserves_missing_file_race() {
    let directory = live_fixture("12\n", "high 1\nmax 0\noom 0\noom_kill 0\n");
    assert_error(check_live(&directory, HIGH));
    fs::remove_file(directory.join("memory.peak")).unwrap();
    let error = check_live(&directory, HIGH).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn live_check_rejects_missing_and_malformed_pids_events() {
    let missing = live_fixture("12\n", standard_events());
    fs::remove_file(missing.join("pids.events")).unwrap();
    assert_error(check_live(&missing, HIGH));
    fs::remove_dir_all(missing).unwrap();

    let malformed = live_fixture("12\n", standard_events());
    fs::write(malformed.join("pids.events"), "max\n").unwrap();
    assert_error(check_live(&malformed, HIGH));
    fs::remove_dir_all(malformed).unwrap();

    let nonzero = live_fixture("12\n", standard_events());
    fs::write(nonzero.join("pids.events"), "max 1\n").unwrap();
    assert_error(check_live(&nonzero, HIGH));
    fs::remove_dir_all(nonzero).unwrap();
}
