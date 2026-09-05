#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use super::super::{acquire_at, current_uid, ensure_lock_directory, open_lock_file};
    use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct LockFixture {
        directory: PathBuf,
        path: PathBuf,
    }

    impl LockFixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let directory = PathBuf::from(format!(
                "/tmp/hardgate-lease-test-{}-{id}",
                std::process::id()
            ));
            let path = directory.join("slot.lock");
            Self { directory, path }
        }

        fn create_directory(&self) {
            let mut builder = DirBuilder::new();
            builder.mode(0o700);
            builder
                .create(&self.directory)
                .expect("fixture directory should be created");
        }

        fn create_file(&self, path: &Path) -> File {
            OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(path)
                .expect("fixture file should be created")
        }

        fn acquire(&self, child_marker: bool) -> io::Result<super::super::MutationLease> {
            acquire_at(
                &self.path,
                current_uid(),
                Instant::now() + Duration::from_secs(2),
                child_marker,
            )
        }
    }

    impl Drop for LockFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn error_from<T>(result: io::Result<T>, message: &str) -> io::Error {
        match result {
            Ok(_) => panic!("{message}"),
            Err(error) => error,
        }
    }

    #[test]
    fn two_file_handles_observe_the_same_exclusive_lock() {
        let fixture = LockFixture::new();
        fixture.create_directory();
        let first = fixture.create_file(&fixture.path);
        first.try_lock().expect("first handle should lock");
        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&fixture.path)
            .expect("second handle should open");

        assert!(matches!(
            second
                .try_lock()
                .expect_err("second handle must observe the held lock"),
            TryLockError::WouldBlock
        ));
    }

    #[test]
    fn same_thread_reentrancy_and_release_are_bounded() {
        let fixture = LockFixture::new();
        let first = fixture.acquire(false).expect("first lease should acquire");
        let second = fixture
            .acquire(false)
            .expect("same-thread lease should be reentrant");
        drop(second);

        let path = fixture.path.clone();
        let uid = current_uid();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let waiter = thread::spawn(move || {
            ready_tx.send(()).expect("waiter should start");
            acquire_at(&path, uid, Instant::now() + Duration::from_secs(2), false).map(drop)
        });
        ready_rx.recv().expect("waiter should report readiness");
        thread::sleep(Duration::from_millis(25));
        drop(first);

        waiter
            .join()
            .expect("waiter should join")
            .expect("released lock should be acquired");
    }

    #[test]
    fn symlink_lock_path_is_rejected() {
        let fixture = LockFixture::new();
        fixture.create_directory();
        let target = fixture.directory.join("symlink-target");
        let _target_file = fixture.create_file(&target);
        std::os::unix::fs::symlink(&target, &fixture.path).expect("symlink should be created");

        let error = error_from(fixture.acquire(false), "symlink lock path must be rejected");
        assert!(error.to_string().contains("symlink"), "{error}");
    }

    #[test]
    fn private_lock_path_operations_report_filesystem_errors() {
        let fixture = LockFixture::new();
        fixture.create_directory();

        let blocker = fixture.directory.join("blocker");
        drop(fixture.create_file(&blocker));
        let blocked_path = blocker.join("child").join("slot.lock");
        let error = error_from(
            ensure_lock_directory(&blocked_path, current_uid()),
            "a non-directory path component must reject lock-directory creation",
        );
        assert!(
            error.to_string().contains("while create lock directory"),
            "{error}"
        );

        let target = fixture.directory.join("target");
        drop(fixture.create_file(&target));
        let link = fixture.directory.join("link");
        std::os::unix::fs::symlink(&target, &link).expect("symlink should be created");
        let error = error_from(
            open_lock_file(&link),
            "NOFOLLOW should reject a symlink when opening the lock file",
        );
        assert!(
            error.to_string().contains("while open lock file"),
            "{error}"
        );
    }

    #[test]
    fn hardlinked_lock_path_is_rejected() {
        let fixture = LockFixture::new();
        fixture.create_directory();
        let target = fixture.directory.join("hardlink-target");
        let target_file = fixture.create_file(&target);
        drop(target_file);
        fs::hard_link(&target, &fixture.path).expect("hard link should be created");

        let error = error_from(
            fixture.acquire(false),
            "hardlinked lock path must be rejected",
        );
        assert!(error.to_string().contains("hard links"), "{error}");
    }

    #[test]
    fn stale_unlocked_file_is_accepted_and_retained() {
        let fixture = LockFixture::new();
        fixture.create_directory();
        let stale_file = fixture.create_file(&fixture.path);
        drop(stale_file);

        let lease = fixture
            .acquire(false)
            .expect("stale unlocked lock file should be accepted");
        drop(lease);

        let metadata = fs::symlink_metadata(&fixture.path).expect("lock file should be retained");
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(metadata.nlink(), 1);
    }

    #[test]
    fn child_marker_rejects_nested_mutation_immediately() {
        let fixture = LockFixture::new();
        let error = error_from(
            fixture.acquire(true),
            "nested mutation marker must reject acquisition",
        );
        assert!(
            error.to_string().contains("HARDGATE_MUTATION_CHILD"),
            "{error}"
        );
    }
}
