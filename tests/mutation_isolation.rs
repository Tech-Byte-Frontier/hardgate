#![cfg(any(target_os = "linux", target_os = "macos"))]

#[path = "support/fs.rs"]
mod fixture_fs;

use rustix::process::{Pid, Signal, kill_process};
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const DIRTY_SOURCE: &str =
    "// current uncommitted input\npub fn compute(value: i32) -> i32 { value + 1 }\n";
const CONFIG: &str = "[gate]\npreset = 'custom'\n[mutation]\nenabled = true\nmin_score = 85.0\n";
static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

struct RunningMutation {
    child: Child,
    root: PathBuf,
    observed: PathBuf,
}

impl RunningMutation {
    fn start(mode: &str, timeout: &str) -> Self {
        Self::start_with(mode, timeout, |_| {})
    }

    fn start_with(mode: &str, timeout: &str, setup: impl FnOnce(&std::path::Path)) -> Self {
        let id = NEXT_CASE.fetch_add(1, Ordering::Relaxed);
        let root = fixture_fs::tempdir(&format!("mutation-isolation-input-{id}"));
        let observed = fixture_fs::tempdir(&format!("mutation-isolation-observed-{id}"));
        fs::write(root.join("sample.rs"), DIRTY_SOURCE).unwrap();
        fs::write(root.join("untracked.txt"), DIRTY_SOURCE).unwrap();
        fs::write(root.join("hardgate.toml"), CONFIG).unwrap();
        let script = format!(
            "printf '%s' \"$PWD\" > '{out}/workspace'\nprintf '%s' \"$CARGO_TARGET_DIR\" > '{out}/target'\nif cmp -s sample.rs untracked.txt; then exit 0; fi\necho $$ > '{out}/pid'\n{action}\n",
            out = observed.display(),
            action = if mode == "sleep" {
                "exec sleep 60"
            } else {
                "echo 'assertion failed'; exit 1"
            }
        );
        fs::write(root.join("test.sh"), script).unwrap();
        setup(&root);
        let child = Command::new(env!("CARGO_BIN_EXE_hardgate"))
            .args([
                "mutate",
                "--scoped",
                "sample.rs",
                "--test-cmd",
                "sh test.sh",
                "--max-mutants",
                "1",
                "--timeout",
                timeout,
                "--json",
            ])
            .env("CARGO_TARGET_DIR", root.join("must-not-build-here"))
            .current_dir(&root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            child,
            root,
            observed,
        }
    }

    fn wait_for_mutant(&mut self) -> (PathBuf, Pid) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.observed.join("pid").exists() {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "mutation exited before observing the isolated mutant"
            );
            assert!(Instant::now() < deadline, "mutant was never executed");
            thread::sleep(Duration::from_millis(10));
        }
        let snapshot = PathBuf::from(fs::read_to_string(self.observed.join("workspace")).unwrap());
        let pid = fs::read_to_string(self.observed.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_ne!(snapshot, self.root);
        assert_eq!(
            fs::read_to_string(self.root.join("sample.rs")).unwrap(),
            DIRTY_SOURCE
        );
        assert_eq!(
            fs::read_to_string(self.observed.join("target")).unwrap(),
            snapshot.join("target").to_string_lossy()
        );
        (snapshot, Pid::from_raw(pid).unwrap())
    }

    fn wait(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "mutation failed to terminate");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for RunningMutation {
    fn drop(&mut self) {
        if let Ok(text) = fs::read_to_string(self.observed.join("pid"))
            && let Ok(number) = text.trim().parse()
            && let Some(pid) = Pid::from_raw(number)
        {
            let _ = kill_process(pid, Signal::KILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Ok(snapshot) = fs::read_to_string(self.observed.join("workspace")) {
            let _ = fs::remove_dir_all(snapshot);
        }
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.observed);
    }
}

#[test]
fn termination_signals_stop_mutation_and_remove_the_disposable_workspace() {
    for signal in [Signal::INT, Signal::TERM] {
        let mut run = RunningMutation::start("sleep", "30");
        let (snapshot, pid) = run.wait_for_mutant();
        assert_ne!(
            fs::read_to_string(snapshot.join("sample.rs")).unwrap(),
            DIRTY_SOURCE
        );
        kill_process(Pid::from_child(&run.child), signal).unwrap();
        run.wait();
        let status = run.child.try_wait().unwrap().unwrap();
        assert_eq!(status.code(), Some(128 + signal.as_raw()));
        assert!(!snapshot.exists());
        assert_eq!(
            rustix::process::test_kill_process(pid),
            Err(rustix::io::Errno::SRCH)
        );
        assert_eq!(
            fs::read_to_string(run.root.join("sample.rs")).unwrap(),
            DIRTY_SOURCE
        );
    }
}

#[test]
fn sigkill_cannot_damage_original_source() {
    let mut run = RunningMutation::start("sleep", "30");
    let (snapshot, _) = run.wait_for_mutant();
    kill_process(Pid::from_child(&run.child), Signal::KILL).unwrap();
    run.wait();
    assert!(snapshot.exists(), "SIGKILL cannot execute cleanup");
    assert_eq!(
        fs::read_to_string(run.root.join("sample.rs")).unwrap(),
        DIRTY_SOURCE
    );
}

#[test]
fn passing_and_timeout_runs_preserve_dirty_input_and_cleanup() {
    for (mode, timeout, success) in [("fail", "30", true), ("sleep", "1", false)] {
        let mut run = RunningMutation::start(mode, timeout);
        let (snapshot, _) = run.wait_for_mutant();
        run.wait();
        assert_eq!(run.child.try_wait().unwrap().unwrap().success(), success);
        assert!(!snapshot.exists());
        assert!(!run.root.join("must-not-build-here").exists());
        assert_eq!(
            fs::read_to_string(run.root.join("sample.rs")).unwrap(),
            DIRTY_SOURCE
        );
    }
}

#[test]
fn internal_dependencies_are_copied_and_later_user_edits_are_preserved() {
    let mut run = RunningMutation::start_with("sleep", "30", |root| {
        fs::create_dir_all(root.join("node_modules/local")).unwrap();
        fs::write(
            root.join("node_modules/local/input.txt"),
            "dependency bytes",
        )
        .unwrap();
        std::os::unix::fs::symlink(root.join("node_modules/local"), root.join("alias")).unwrap();
        for omitted in ["target", ".git"] {
            fs::create_dir_all(root.join(omitted)).unwrap();
            fs::write(root.join(omitted).join("sentinel"), "omit this output").unwrap();
        }
    });
    let (snapshot, _) = run.wait_for_mutant();
    assert!(!snapshot.join("target/sentinel").exists());
    assert!(!snapshot.join(".git").exists());
    assert!(
        snapshot
            .join("alias")
            .canonicalize()
            .unwrap()
            .starts_with(&snapshot)
    );
    fs::write(snapshot.join("alias/input.txt"), "changed copy").unwrap();
    assert_eq!(
        fs::read_to_string(run.root.join("node_modules/local/input.txt")).unwrap(),
        "dependency bytes"
    );
    fs::write(run.root.join("sample.rs"), "later user edit").unwrap();
    kill_process(Pid::from_child(&run.child), Signal::TERM).unwrap();
    run.wait();
    assert_eq!(
        fs::read_to_string(run.root.join("sample.rs")).unwrap(),
        "later user edit"
    );
}

fn rejected_snapshot(link: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .args([
            "mutate",
            "--scoped",
            "sample.rs",
            "--test-cmd",
            "true",
            "--json",
        ])
        .current_dir(link)
        .output()
        .unwrap()
}

#[test]
fn external_symlinks_fail_before_tests_or_source_mutation() {
    let root = fixture_fs::tempdir("mutation-external-link");
    let outside = fixture_fs::tempdir("mutation-external-target");
    fs::write(root.join("sample.rs"), DIRTY_SOURCE).unwrap();
    fs::write(root.join("hardgate.toml"), CONFIG).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("external")).unwrap();
    let output = rejected_snapshot(&root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("leaves the workspace"));
    assert_eq!(
        fs::read_to_string(root.join("sample.rs")).unwrap(),
        DIRTY_SOURCE
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn temporary_directory_inside_source_is_rejected_without_recursive_copy() {
    let root = fixture_fs::tempdir("mutation-contained-temp");
    let temporary = root.join("tmp");
    fs::create_dir(&temporary).unwrap();
    fs::write(root.join("sample.rs"), DIRTY_SOURCE).unwrap();
    fs::write(root.join("hardgate.toml"), CONFIG).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .args([
            "mutate",
            "--scoped",
            "sample.rs",
            "--test-cmd",
            "true",
            "--json",
        ])
        .env("TMPDIR", &temporary)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("outside the source workspace"));
    assert_eq!(fs::read_dir(&temporary).unwrap().count(), 0);
    assert_eq!(
        fs::read_to_string(root.join("sample.rs")).unwrap(),
        DIRTY_SOURCE
    );
    fs::remove_dir_all(root).unwrap();
}
