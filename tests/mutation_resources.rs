#![cfg(any(target_os = "linux", target_os = "macos"))]

#[path = "support/fs.rs"]
mod fixture_fs;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const MUTATION_CONFIG: &str = r#"[gate]
preset = "custom"
strict = true

[coverage]
enabled = false

[mutation]
enabled = true
min_score = 100.0
timeout_secs = 5
"#;
const SOURCE: &str = "pub fn answer() -> bool { true }\n";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = fixture_fs::tempdir(&format!("mutation-resources-{tag}-{id}"));
        write(&root, "hardgate.toml", MUTATION_CONFIG);
        write(&root, "src/lib.rs", SOURCE);
        Self { root }
    }

    fn write_verify_script(&self, body: &str) {
        write(&self.root, "verify.sh", body);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct RunningCli {
    child: Option<Child>,
    root: PathBuf,
}

impl RunningCli {
    fn spawn(root: &Path) -> Self {
        let child = mutation_command(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("hardgate mutation should spawn");
        Self {
            child: Some(child),
            root: root.to_path_buf(),
        }
    }

    fn wait(&mut self) -> Output {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = self
                .child
                .as_mut()
                .expect("running CLI child should exist")
                .try_wait()
                .expect("CLI child should be waitable");
            if status.is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "hardgate mutation did not terminate"
            );
            thread::sleep(Duration::from_millis(10));
        }
        self.child
            .take()
            .expect("CLI child should exist")
            .wait_with_output()
            .expect("CLI child output should be collected")
    }
}

impl Drop for RunningCli {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture parent should be created");
    }
    fs::write(path, content).expect("fixture file should be written");
}

fn mutation_command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hardgate"));
    command
        .args([
            "mutate",
            "--scoped",
            "src/lib.rs",
            "--test-cmd",
            "sh verify.sh",
            "--max-mutants",
            "1",
            "--timeout",
            "5",
        ])
        .current_dir(root);
    command
}

fn run_mutation(root: &Path, environment: &[(&str, &str)]) -> Output {
    let mut command = mutation_command(root);
    for &(name, value) in environment {
        command.env(name, value);
    }
    command.output().expect("hardgate mutation should run")
}

fn diagnostic(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assertion_script(marker: Option<&Path>) -> String {
    let marker_write = marker.map_or_else(String::new, |path| {
        format!("printf 'ran\\n' > '{}'\n", path.display())
    });
    format!(
        "{marker_write}if grep -q 'true' src/lib.rs; then exit 0; fi\nprintf 'assertion failed\\n'\nexit 1\n"
    )
}

fn worker_script(log: &Path) -> String {
    format!(
        "printf '%s,%s,%s\\n' \"${{CARGO_BUILD_JOBS-}}\" \"${{RUST_TEST_THREADS-}}\" \"${{RAYON_NUM_THREADS-}}\" >> '{}'\nif [ \"${{CARGO_BUILD_JOBS:-0}}\" -gt 2 ] || [ \"${{RUST_TEST_THREADS:-0}}\" -gt 2 ] || [ \"${{RAYON_NUM_THREADS:-0}}\" -gt 2 ]; then exit 3; fi\nif grep -q 'true' src/lib.rs; then exit 0; fi\nprintf 'assertion failed\\n'\nexit 1\n",
        log.display()
    )
}

fn concurrent_script(label: &str, log: &Path) -> String {
    format!(
        "printf '%s-start\\n' '{label}' >> '{log}'\nsleep 0.2\nif grep -q 'true' src/lib.rs; then status=0; else printf 'assertion failed\\n'; status=1; fi\nprintf '%s-end\\n' '{label}' >> '{log}'\nexit \"$status\"\n",
        label = label,
        log = log.display()
    )
}

fn wait_for_log(path: &Path, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::read_to_string(path)
            .map(|contents| contents.contains(needle))
            .unwrap_or(false)
        {
            return;
        }
        assert!(Instant::now() < deadline, "log never contained `{needle}`");
        thread::sleep(Duration::from_millis(10));
    }
}

fn worker_values(log: &Path) -> Vec<[u32; 3]> {
    fs::read_to_string(log)
        .expect("worker log should exist")
        .lines()
        .map(|line| {
            let values = line
                .split(',')
                .map(|value| {
                    value
                        .parse::<u32>()
                        .expect("worker value should be numeric")
                })
                .collect::<Vec<_>>();
            assert_eq!(values.len(), 3, "worker log line should have three values");
            [values[0], values[1], values[2]]
        })
        .collect()
}

#[test]
fn inherited_child_marker_fails_before_snapshot_or_test_command() {
    let fixture = Fixture::new("nested");
    let marker = fixture.root.join("test-command-ran");
    fixture.write_verify_script(&assertion_script(Some(&marker)));

    let output = run_mutation(&fixture.root, &[("HARDGATE_MUTATION_CHILD", "1")]);

    assert_eq!(output.status.code(), Some(2), "{}", diagnostic(&output));
    assert!(
        diagnostic(&output).contains("nested mutation"),
        "{}",
        diagnostic(&output)
    );
    assert!(!marker.exists(), "test command ran despite nested marker");
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/lib.rs")).unwrap(),
        SOURCE
    );
}

#[test]
fn mutation_test_children_are_capped_and_explicit_one_is_preserved() {
    let defaults = Fixture::new("workers-default");
    let default_log = defaults.root.join("workers.log");
    fs::write(&default_log, "").unwrap();
    defaults.write_verify_script(&worker_script(&default_log));
    let inherited = [
        ("CARGO_BUILD_JOBS", "64"),
        ("RUST_TEST_THREADS", "64"),
        ("RAYON_NUM_THREADS", "64"),
    ];
    let output = run_mutation(&defaults.root, &inherited);
    assert!(output.status.success(), "{}", diagnostic(&output));
    let values = worker_values(&default_log);
    assert!(!values.is_empty(), "worker command should have run");
    assert!(
        values
            .iter()
            .all(|row| row.iter().all(|value| (1..=2).contains(value)))
    );
    assert_eq!(
        fs::read_to_string(defaults.root.join("src/lib.rs")).unwrap(),
        SOURCE
    );

    let explicit = Fixture::new("workers-explicit");
    let explicit_log = explicit.root.join("workers.log");
    fs::write(&explicit_log, "").unwrap();
    explicit.write_verify_script(&worker_script(&explicit_log));
    let explicit_one = [
        ("CARGO_BUILD_JOBS", "1"),
        ("RUST_TEST_THREADS", "1"),
        ("RAYON_NUM_THREADS", "1"),
    ];
    let output = run_mutation(&explicit.root, &explicit_one);
    assert!(output.status.success(), "{}", diagnostic(&output));
    let values = worker_values(&explicit_log);
    assert!(
        !values.is_empty(),
        "explicit worker command should have run"
    );
    assert!(values.iter().all(|row| row == &[1, 1, 1]));
    assert_eq!(
        fs::read_to_string(explicit.root.join("src/lib.rs")).unwrap(),
        SOURCE
    );
}

#[test]
fn concurrent_cli_runs_hold_the_lease_for_their_whole_mutation() {
    let shared = Fixture::new("shared-log");
    let log = shared.root.join("runs.log");
    fs::write(&log, "").unwrap();
    let first = Fixture::new("concurrent-a");
    first.write_verify_script(&concurrent_script("A", &log));
    let second = Fixture::new("concurrent-b");
    second.write_verify_script(&concurrent_script("B", &log));

    let mut first_run = RunningCli::spawn(&first.root);
    wait_for_log(&log, "A-start");
    let mut second_run = RunningCli::spawn(&second.root);
    let first_output = first_run.wait();
    let second_output = second_run.wait();
    assert!(
        first_output.status.success(),
        "{}",
        diagnostic(&first_output)
    );
    assert!(
        second_output.status.success(),
        "{}",
        diagnostic(&second_output)
    );

    let events = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(
        events.len(),
        8,
        "each run should emit baseline and mutant pairs"
    );
    assert!(
        events[..4].iter().all(|event| event.starts_with('A'))
            || events[..4].iter().all(|event| event.starts_with('B'))
    );
    assert!(
        events[4..].iter().all(|event| event.starts_with('A'))
            || events[4..].iter().all(|event| event.starts_with('B'))
    );
    assert_ne!(events[0].chars().next(), events[4].chars().next());
    assert_eq!(
        events
            .iter()
            .filter(|event| event.as_str() == "A-start" || event.as_str() == "A-end")
            .count(),
        4
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.as_str() == "B-start" || event.as_str() == "B-end")
            .count(),
        4
    );
    assert_eq!(
        fs::read_to_string(first.root.join("src/lib.rs")).unwrap(),
        SOURCE
    );
    assert_eq!(
        fs::read_to_string(second.root.join("src/lib.rs")).unwrap(),
        SOURCE
    );
}
