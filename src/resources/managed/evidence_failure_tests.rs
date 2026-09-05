use super::*;
use std::fs;
use std::io;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const MEMORY: u64 = 64 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn fixture() -> CommandEvidence {
    CommandEvidence::create(MEMORY).expect("evidence fixture")
}

fn report(evidence: &CommandEvidence) -> String {
    format!(
        "id={}\nlimit={}\nhigh={}\nstatus=0\npeak=12\n\
         pids_max_events=0\nevents.high=0\nevents.max=0\n\
         events.oom=0\nevents.oom_kill=0\n",
        evidence.identity(),
        evidence.memory_bytes,
        evidence.high_bytes,
    )
}

fn complete_report() -> &'static str {
    "id=test\nlimit=1\nhigh=1\nstatus=0\npeak=0\n\
     pids_max_events=0\nevents.high=0\nevents.max=0\n\
     events.oom=0\nevents.oom_kill=0\n"
}

fn exited(code: i32) -> std::process::ExitStatus {
    std::process::ExitStatus::from_raw(code << 8)
}

fn error_text<T>(result: io::Result<T>, expected: &str) {
    let error = match result {
        Ok(_) => panic!("expected evidence failure"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains(expected),
        "expected {expected:?} in {error}"
    );
}

fn error_kind<T>(result: io::Result<T>, expected: io::ErrorKind) {
    let error = match result {
        Ok(_) => panic!("expected evidence failure"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), expected);
}

fn without_line(content: &str, key: &str) -> String {
    content
        .lines()
        .filter(|line| !line.starts_with(key))
        .collect::<Vec<_>>()
        .join("\n")
}

fn temp_entry(label: &str) -> PathBuf {
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "hardgate-evidence-failure-{}-{label}-{id}",
        std::process::id()
    ))
}

fn private_directory(label: &str) -> PathBuf {
    let directory = temp_entry(label);
    fs::create_dir(&directory).expect("create private fixture directory");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .expect("protect fixture directory");
    directory
}

#[test]
fn rejects_zero_memory_budget_before_directory_creation() {
    error_text(CommandEvidence::create(0), "memory.high limit is zero");
}

#[test]
fn write_shim_rejects_an_existing_target() {
    let path = temp_entry("existing-shim");
    fs::write(&path, b"existing").unwrap();
    error_text(write_shim(&path), "create mutation evidence shim");
    fs::remove_file(path).unwrap();
}

#[test]
fn rejects_each_missing_typed_report_field() {
    for (key, message) in [
        ("id=", "missing id"),
        ("limit=", "missing limit"),
        ("high=", "missing high"),
        ("status=", "missing status"),
        ("peak=", "missing peak"),
        ("pids_max_events=", "missing pids_max_events"),
    ] {
        error_text(parse_report(&without_line(complete_report(), key)), message);
    }
}

#[test]
fn rejects_report_syntax_unknown_and_invalid_events() {
    for content in [
        "malformed",
        "=value\n",
        "id=test\nid=again\n",
        "id=test\nlimit=1\nhigh=1\nstatus=0\npeak=0\n\
         pids_max_events=0\nevents.high=0\nevents.max=0\n\
         events.oom=0\nevents.oom_kill=0\nextra=0\n",
        "id=test\nlimit=1\nhigh=1\nstatus=0\npeak=0\n\
         pids_max_events=0\nevents.high=0\nevents.max=0\n\
         events.oom=0\nevents.oom_kill=0\nevents.bad-key=0\n",
    ] {
        assert!(parse_report(content).is_err(), "accepted malformed report");
    }
}

#[test]
fn rejects_invalid_and_overflowing_report_numbers() {
    for (field, value) in [
        ("limit", "bad"),
        ("high", "-1"),
        ("status", ""),
        ("peak", "18446744073709551616"),
        ("pids_max_events", "+1"),
    ] {
        let content = complete_report()
            .lines()
            .map(|line| {
                if line.starts_with(field) {
                    format!("{field}={value}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(parse_report(&content).is_err(), "accepted invalid {field}");
    }
}

#[test]
fn rejects_missing_required_memory_event_counters() {
    for key in ["high", "max", "oom", "oom_kill"] {
        let content = "high 0\nmax 0\noom 0\noom_kill 0\n";
        let content = without_line(content, key);
        error_text(parse_events(&content), "memory.events is missing");
    }
}

#[test]
fn rejects_malformed_memory_event_rows() {
    for content in [
        "\n",
        "high\nmax 0\noom 0\noom_kill 0\n",
        "high 0 extra\nmax 0\noom 0\noom_kill 0\n",
        "bad-key 0\nmax 0\noom 0\noom_kill 0\n",
        "high bad\nmax 0\noom 0\noom_kill 0\n",
        "high 0\nhigh 0\nmax 0\noom 0\noom_kill 0\n",
    ] {
        assert!(parse_events(content).is_err(), "accepted malformed events");
    }
}

#[test]
fn rejects_malformed_pids_event_rows_and_nonzero_max() {
    for content in [
        "",
        "max 0\nmax 0\n",
        "other 0\n",
        "max\n",
        "max 0 extra\n",
        "max bad\n",
        "max 1\n",
    ] {
        assert!(
            parse_pids_events(content).is_err(),
            "accepted malformed pids events"
        );
    }
}

#[test]
fn rejects_invalid_directory_targets_and_protects_cleanup() {
    let missing = temp_entry("missing-directory");
    error_kind(verify_directory(&missing), io::ErrorKind::NotFound);
    error_kind(protect_directory(&missing), io::ErrorKind::NotFound);

    let file = temp_entry("regular-file");
    fs::write(&file, b"fixture").expect("write regular fixture");
    error_text(verify_directory(&file), "not a private owned directory");
    error_text(protect_directory(&file), "not a private owned directory");
    assert!(file.exists(), "failed cleanup must not remove regular file");
    fs::remove_file(file).unwrap();

    let directory = private_directory("symlink-target");
    let link = temp_entry("symlink-directory");
    symlink(&directory, &link).expect("create directory symlink");
    error_text(verify_directory(&link), "not a private owned directory");
    fs::remove_file(link).unwrap();
    fs::remove_dir(directory).unwrap();
}

#[test]
fn rejects_directory_with_wrong_permissions() {
    let directory = private_directory("wrong-mode");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).expect("make mode invalid");
    error_text(
        verify_directory(&directory),
        "not a private owned directory",
    );
    fs::remove_dir(directory).unwrap();
}

#[test]
fn create_directory_attempt_handles_collision_and_invalid_root() {
    let root = private_directory("collision-root");
    let mut collided = false;
    for _ in 0..128 {
        let counter = NEXT_ID.load(Ordering::Relaxed);
        let candidate = root.join(format!(
            "hardgate-resource-{}-{counter}",
            std::process::id()
        ));
        fs::create_dir(&candidate).expect("reserve candidate directory");
        match create_directory_attempt(&root).expect("try candidate directory") {
            None => {
                collided = true;
                fs::remove_dir(candidate).unwrap();
                break;
            }
            Some((created, _)) => {
                fs::remove_dir(created).unwrap();
                fs::remove_dir(candidate).unwrap();
            }
        }
    }
    assert!(collided, "did not observe a reserved directory collision");
    fs::remove_dir(root).unwrap();

    let invalid_root = temp_entry("invalid-root");
    fs::write(&invalid_root, b"not a directory").unwrap();
    assert!(create_directory_attempt(&invalid_root).is_err());
    fs::remove_file(invalid_root).unwrap();
}

#[test]
fn allow_start_rejects_pending_marker_and_missing_parent() {
    let pending = fixture();
    let pending_path = pending.report_path().with_extension("start-pending");
    fs::write(&pending_path, b"reserved").unwrap();
    error_kind(pending.allow_start(), io::ErrorKind::AlreadyExists);

    let missing = fixture();
    let directory = missing.shim_path().parent().unwrap().to_path_buf();
    fs::remove_dir_all(directory).unwrap();
    error_kind(missing.allow_start(), io::ErrorKind::NotFound);
}

#[test]
fn read_report_rejects_invalid_utf8_directories_and_hard_links() {
    let evidence = fixture();
    fs::write(evidence.report_path(), [0xff_u8, 0xfe]).unwrap();
    error_text(evidence.verify(&exited(0)), "not UTF-8");

    let directory_report = fixture();
    fs::create_dir(directory_report.report_path()).unwrap();
    error_text(
        directory_report.verify(&exited(0)),
        "not a regular single-link file",
    );

    let linked = fixture();
    let target = linked.report_path().with_extension("target");
    fs::write(&target, report(&linked)).unwrap();
    fs::hard_link(&target, linked.report_path()).unwrap();
    error_text(linked.verify(&exited(0)), "not a regular single-link file");
}

#[test]
fn read_bounded_preserves_missing_and_wraps_read_errors() {
    let missing = temp_entry("missing-report");
    assert_eq!(
        read_bounded(&missing).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );

    let directory = private_directory("read-directory");
    error_text(read_bounded(&directory), "read mutation evidence");
    fs::remove_dir(directory).unwrap();
}

#[test]
fn read_kernel_file_rejects_non_utf8_and_parse_decimal_is_strict() {
    let path = temp_entry("invalid-kernel");
    fs::write(&path, [0xff_u8]).unwrap();
    error_text(read_kernel_file(&path), "not UTF-8");
    fs::remove_file(path).unwrap();

    for value in ["", "-1", "bad", "18446744073709551616"] {
        assert!(parse_decimal(value, "fixture").is_err());
    }
    assert_eq!(parse_decimal("12", "fixture").unwrap(), 12);
}

#[test]
fn resource_io_preserves_not_found_and_wraps_other_errors() {
    let not_found = resource_io("read", io::Error::from(io::ErrorKind::NotFound));
    assert_eq!(not_found.kind(), io::ErrorKind::NotFound);
    let wrapped = resource_io("read", io::Error::other("broken"));
    assert!(wrapped.to_string().contains("read: broken"));
}
