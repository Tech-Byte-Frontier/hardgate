use super::*;

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    scratch: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = crate::fs_tests::tempdir("managed-workspace");
        let source = root.join("source");
        let scratch = root.join("scratch");
        fs::create_dir_all(source.join("node_modules/package")).unwrap();
        fs::create_dir(&scratch).unwrap();
        fs::write(source.join("source.ts"), "export const value = 42;").unwrap();
        fs::write(
            source.join("node_modules/package/index.js"),
            "module.exports=42",
        )
        .unwrap();
        fs::write(scratch.join("unrelated-backup"), "keep").unwrap();
        Self {
            root,
            source,
            scratch,
        }
    }
    fn workspace(&self) -> EvidenceWorkspace {
        EvidenceWorkspace::create_at(&self.source, &self.scratch).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn successful_completion_removes_copied_dependencies_and_build_caches() {
    let f = Fixture::new();
    let workspace = f.workspace();
    let job = workspace.job_path().to_path_buf();
    assert!(
        workspace
            .root()
            .join("node_modules/package/index.js")
            .exists()
    );
    fs::create_dir_all(workspace.root().join("target/debug")).unwrap();
    fs::write(workspace.root().join("target/debug/cache"), "build").unwrap();
    workspace.close().unwrap();
    assert!(!job.exists());
    assert!(f.source.join("source.ts").exists());
    assert!(f.scratch.join("unrelated-backup").exists());
}

#[test]
fn active_workspace_holds_a_lock_and_has_no_completion_marker() {
    let f = Fixture::new();
    let workspace = f.workspace();
    let job = workspace.job_path().to_path_buf();
    let lock = fs::File::open(job.join(".lock")).unwrap();
    assert!(matches!(lock.try_lock(), Err(fs::TryLockError::WouldBlock)));
    assert!(!job.join(".completed").exists());
    workspace.failed("producer failed").unwrap();
    workspace.preserve().unwrap();
    lock.try_lock().unwrap();
    assert!(job.join("work/source.ts").exists());
    let state = fs::read_to_string(job.join("lifecycle.json")).unwrap();
    assert!(state.contains("failed"));
}

#[test]
fn unfinished_operation_retains_diagnostics_and_original_inputs() {
    let f = Fixture::new();
    let workspace = f.workspace();
    let job = workspace.job_path().to_path_buf();
    workspace
        .diagnostics("baseline", "test failed at an assertion")
        .unwrap();
    drop(workspace);
    assert!(job.join("work/source.ts").exists());
    assert!(
        fs::read_to_string(job.join("diagnostics.log"))
            .unwrap()
            .contains("test failed")
    );
    assert!(
        fs::read_to_string(job.join("lifecycle.json"))
            .unwrap()
            .contains("failed")
    );
    assert!(!job.join(".completed").exists());
}

#[test]
fn cleanup_refuses_replaced_jobs_and_scratch_inside_source() {
    let f = Fixture::new();
    assert!(EvidenceWorkspace::create_at(&f.source, &f.source).is_err());
    let workspace = f.workspace();
    let job = workspace.job_path().to_path_buf();
    let moved = f.scratch.join("retained-original");
    fs::rename(&job, &moved).unwrap();
    fs::create_dir(&job).unwrap();
    fs::write(job.join("user-file"), "preserve").unwrap();
    assert!(
        workspace
            .close()
            .unwrap_err()
            .to_string()
            .contains("identity changed")
    );
    assert_eq!(
        fs::read_to_string(job.join("user-file")).unwrap(),
        "preserve"
    );
    assert!(moved.join("work/source.ts").exists());
}

#[cfg(unix)]
#[test]
fn cleanup_failure_has_no_completed_marker_and_retains_diagnostics() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let workspace = f.workspace();
    let job = workspace.job_path().to_path_buf();
    let work = workspace.root().to_path_buf();
    fs::set_permissions(&work, fs::Permissions::from_mode(0o000)).unwrap();
    let result = workspace.close();
    fs::set_permissions(&work, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert!(!job.join(".completed").exists());
    assert!(
        fs::read_to_string(job.join("lifecycle.json"))
            .unwrap()
            .contains("cleanup-failed")
    );
    assert!(work.join("source.ts").exists());
}

#[cfg(unix)]
#[test]
fn scratch_alias_cannot_create_directories_inside_source() {
    let f = Fixture::new();
    let alias = f.root.join("alias");
    std::os::unix::fs::symlink(&f.source, &alias).unwrap();
    assert!(EvidenceWorkspace::create_at(&f.source, &alias.join("new/scratch")).is_err());
    assert!(!f.source.join("new").exists());
}

#[test]
fn cleanup_refuses_a_replaced_copy_without_touching_either_directory() {
    let f = Fixture::new();
    let workspace = f.workspace();
    let work = workspace.root().to_path_buf();
    let moved = f.scratch.join("original-copy");
    fs::rename(&work, &moved).unwrap();
    fs::create_dir(&work).unwrap();
    fs::write(work.join("unrelated"), "preserve").unwrap();
    assert!(workspace.close().is_err());
    assert!(work.join("unrelated").is_file());
    assert!(moved.join("source.ts").is_file());
}

#[cfg(unix)]
#[test]
fn cleanup_refuses_lock_replacement_and_directory_symlinks() {
    let f = Fixture::new();
    for symlink in [false, true] {
        let workspace = f.workspace();
        let lock = workspace.job_path().join(".lock");
        let original = workspace.job_path().join("original-lock");
        fs::rename(&lock, &original).unwrap();
        if symlink {
            std::os::unix::fs::symlink(&original, &lock).unwrap();
        } else {
            fs::write(&lock, "replacement").unwrap();
        }
        let work = workspace.root().to_path_buf();
        assert!(workspace.close().is_err());
        assert!(work.join("source.ts").exists());
    }
    for job in [false, true] {
        let workspace = f.workspace();
        let replaced = if job {
            workspace.job_path()
        } else {
            workspace.root()
        }
        .to_path_buf();
        let moved = f
            .scratch
            .join(if job { "original-job" } else { "original-work" });
        fs::rename(&replaced, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &replaced).unwrap();
        assert!(workspace.close().is_err());
        assert!(moved.is_dir());
    }
}
