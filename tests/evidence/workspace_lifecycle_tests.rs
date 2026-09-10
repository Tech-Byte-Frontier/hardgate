use super::protocol_tests::{Project, assert_exit, lcov};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Scratch(PathBuf);
impl Scratch {
    fn new(project: &Project) -> Self {
        let path = project.0.with_extension("managed-scratch");
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn job(&self) -> PathBuf {
        let mut jobs = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().path());
        let job = jobs.next().expect("one managed job");
        assert!(jobs.next().is_none());
        job
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn state(job: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(job.join("lifecycle.json")).unwrap()).unwrap()
}

#[test]
fn success_publishes_external_evidence_and_removes_the_managed_copy() {
    let project = Project::new();
    let scratch = Scratch::new(&project);
    let output = project
        .producer_command("vitest", lcov(), ("pass", 0))
        .arg("--scratch-root")
        .arg(&scratch.0)
        .output()
        .unwrap();
    assert_exit(&output, 0);
    assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 0);
    assert!(project.report("coverage").is_file());
    project
        .verify(hardgate::evidence::EvidenceKind::Coverage)
        .unwrap();
}

#[test]
fn failed_producer_keeps_its_copy_and_diagnostics_under_tmpdir() {
    let project = Project::new();
    let scratch = Scratch::new(&project);
    let output = project
        .producer_command("vitest", lcov(), ("malformed", 0))
        .env_remove("HARDGATE_SCRATCH_ROOT")
        .env("TMPDIR", &scratch.0)
        .output()
        .unwrap();
    assert_exit(&output, 2);
    let job = scratch.job();
    assert_eq!(state(&job)["status"], "publication-failed");
    assert!(job.join("work/src/lib.rs").is_file());
    assert!(job.join("diagnostics.log").is_file());
    assert!(!project.receipt("coverage").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains(&job.display().to_string()));
}

#[test]
fn interrupted_producer_retains_workspace_without_a_receipt_or_live_lock() {
    let project = Project::new();
    let scratch = Scratch::new(&project);
    let tool = project.0.join("node_modules/.bin/vitest");
    std::fs::write(tool, "#!/bin/sh\nif [ \"$1\" = --version ]; then echo fixture-1; exit 0; fi\nprintf started > .hardgate/evidence/run/started\nsleep 30\n").unwrap();
    let mut child = project
        .command()
        .args(["evidence", "vitest", "--timeout-secs", "60"])
        .arg("--scratch-root")
        .arg(&scratch.0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let started = loop {
        if let Some(path) = std::fs::read_dir(&scratch.0)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("work/.hardgate/evidence/run/started"))
            .find(|path| path.is_file())
        {
            break path;
        }
        assert!(Instant::now() < deadline, "producer failed to start");
        assert!(
            child.try_wait().unwrap().is_none(),
            "producer exited before interruption"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(started.is_file());
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let job = scratch.job();
    assert_eq!(
        state(&job)["status"],
        "interrupted",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(job.join("work/src/lib.rs").is_file());
    assert!(!project.receipt("coverage").exists());
    assert!(!job.join(".completed").exists());
    std::fs::File::open(job.join(".lock"))
        .unwrap()
        .try_lock()
        .unwrap();
}
