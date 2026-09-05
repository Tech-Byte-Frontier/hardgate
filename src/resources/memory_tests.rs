#[cfg(target_os = "linux")]
mod linux {
    use super::super::procfs::{parse_meminfo, parse_pressure};
    use super::super::{MemorySample, sample_from_paths};
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        root: PathBuf,
        proc_root: PathBuf,
        mount_point: PathBuf,
    }

    #[derive(Clone, Copy)]
    struct CgroupSettings<'a> {
        relative: &'a str,
        maximum: &'a str,
        high: &'a str,
        current: &'a str,
    }

    impl<'a> CgroupSettings<'a> {
        fn new(relative: &'a str, maximum: &'a str, high: &'a str, current: &'a str) -> Self {
            Self {
                relative,
                maximum,
                high,
                current,
            }
        }
    }

    #[derive(Clone, Copy)]
    struct PressureSettings<'a> {
        relative: &'a str,
        full: f64,
        some: f64,
    }

    impl<'a> PressureSettings<'a> {
        fn new(relative: &'a str, full: f64, some: f64) -> Self {
            Self {
                relative,
                full,
                some,
            }
        }
    }

    impl Fixture {
        fn new(cgroup_path: &str, mount_root: &str) -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "hardgate-memory-fixture-{}-{id}",
                std::process::id()
            ));
            let proc_root = root.join("proc");
            let mount_point = root.join("cgroup");
            fs::create_dir_all(proc_root.join("self")).unwrap();
            fs::create_dir_all(proc_root.join("pressure")).unwrap();
            fs::create_dir_all(&mount_point).unwrap();
            fs::write(
                proc_root.join("meminfo"),
                "MemTotal:       2000 kB\nMemAvailable:   1000 kB\n",
            )
            .unwrap();
            fs::write(
                proc_root.join("pressure/memory"),
                "some avg10=0.50 avg60=0.00 avg300=0.00 total=1\nfull avg10=0.25 avg60=0.00 avg300=0.00 total=1\n",
            )
            .unwrap();
            fs::write(proc_root.join("self/cgroup"), format!("0::{cgroup_path}\n")).unwrap();
            let fixture = Self {
                root,
                proc_root,
                mount_point,
            };
            fixture.set_mountinfo(&format!(
                "42 1 0:42 {mount_root} {} rw - cgroup2 cgroup rw\n",
                fixture.mount_point.display()
            ));
            fixture
        }

        fn set_mountinfo(&self, entries: &str) {
            fs::write(self.proc_root.join("self/mountinfo"), entries).unwrap();
        }

        fn write_cgroup(&self, settings: CgroupSettings<'_>) {
            self.write_cgroup_at(&self.mount_point, settings);
        }

        fn write_cgroup_at(&self, mount_point: &Path, settings: CgroupSettings<'_>) {
            let directory = mount_point.join(settings.relative);
            fs::create_dir_all(&directory).unwrap();
            fs::write(
                directory.join("memory.max"),
                format!("{}\n", settings.maximum),
            )
            .unwrap();
            fs::write(
                directory.join("memory.high"),
                format!("{}\n", settings.high),
            )
            .unwrap();
            fs::write(
                directory.join("memory.current"),
                format!("{}\n", settings.current),
            )
            .unwrap();
        }

        fn remove_cgroup_file(&self, relative: &str, file: &str) {
            fs::remove_file(self.mount_point.join(relative).join(file)).unwrap();
        }

        fn write_pressure(&self, settings: PressureSettings<'_>) {
            self.write_pressure_at(&self.mount_point, settings);
        }

        fn write_pressure_at(&self, mount_point: &Path, settings: PressureSettings<'_>) {
            let directory = mount_point.join(settings.relative);
            fs::create_dir_all(&directory).unwrap();
            fs::write(
                directory.join("memory.pressure"),
                format!(
                    "some avg10={:.2} avg60=0.00 avg300=0.00 total=1\nfull avg10={:.2} avg60=0.00 avg300=0.00 total=1\n",
                    settings.some, settings.full,
                ),
            )
            .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn assert_invalid<T>(result: io::Result<T>) {
        match result {
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{error}"),
            Ok(_) => panic!("fixture should be rejected"),
        }
    }

    #[test]
    fn meminfo_requires_checked_kilobytes_and_both_fields() {
        assert_eq!(
            parse_meminfo("MemTotal: 2 kB\nMemAvailable: 1 kB\n").unwrap(),
            (2048, 1024)
        );
        assert_invalid(parse_meminfo("MemTotal: 2 kB\n"));
        assert_invalid(parse_meminfo(
            "MemTotal: 18446744073709551615 kB\nMemAvailable: 1 kB\n",
        ));
        assert_invalid(parse_meminfo("MemTotal: 1 kB\nMemAvailable: 2 kB\n"));
    }

    #[test]
    fn pressure_requires_both_finite_avg10_values() {
        let pressure = parse_pressure(
            "some avg10=2.5 avg60=0 avg300=0 total=1\nfull avg10=0.5 avg60=0 avg300=0 total=1\n",
        )
        .unwrap();
        assert_eq!(pressure.some_avg10, 2.5);
        assert_eq!(pressure.full_avg10, 0.5);
        assert_invalid(parse_pressure("some avg10=1 avg60=0 avg300=0 total=1\n"));
        assert_invalid(parse_pressure(
            "some avg10=NaN avg60=0 avg300=0 total=1\nfull avg10=0 avg60=0 avg300=0 total=1\n",
        ));
        assert_invalid(parse_pressure(
            "some avg10=-1 avg60=0 avg300=0 total=1\nfull avg10=0 avg60=0 avg300=0 total=1\n",
        ));
        assert_invalid(parse_pressure(
            "some avg10=101 avg60=0 avg300=0 total=1\nfull avg10=0 avg60=0 avg300=0 total=1\n",
        ));
    }

    #[test]
    fn sample_uses_finite_limits_and_pressure_from_each_ancestor() {
        let fixture = Fixture::new("/tenant/job", "/");
        fixture.write_cgroup(CgroupSettings::new("tenant", "921600", "max", "204800"));
        fixture.write_cgroup(CgroupSettings::new(
            "tenant/job",
            "512000",
            "307200",
            "102400",
        ));
        fixture.write_pressure(PressureSettings::new("tenant", 0.75, 4.0));
        fixture.write_pressure(PressureSettings::new("tenant/job", 2.0, 3.0));

        let sample = sample_from_paths(&fixture.proc_root).unwrap();
        assert_eq!(sample.total_bytes, 500 * 1024);
        assert_eq!(sample.available_bytes, 200 * 1024);
        assert_eq!(sample.full_avg10, 2.0);
        assert_eq!(sample.some_avg10, 4.0);
    }

    #[test]
    fn sample_maps_namespace_path_relative_to_mount_root() {
        let fixture = Fixture::new("/tenant/job", "/tenant");
        fixture.write_cgroup(CgroupSettings::new("job", "max", "max", "100"));
        let sample = sample_from_paths(&fixture.proc_root).unwrap();
        assert_eq!(sample.total_bytes, 2000 * 1024);
        assert_eq!(sample.available_bytes, 1000 * 1024);
    }

    #[test]
    fn sample_rejects_partial_root_controller_telemetry() {
        let fixture = Fixture::new("/", "/");
        fs::write(fixture.mount_point.join("memory.max"), "1024\n").unwrap();
        assert_invalid(sample_from_paths(&fixture.proc_root));
    }

    #[test]
    fn sample_rejects_nonexistent_mount_root() {
        let fixture = Fixture::new("/", "/");
        fs::remove_dir_all(&fixture.mount_point).unwrap();
        assert!(sample_from_paths(&fixture.proc_root).is_err());
    }

    #[test]
    fn sample_combines_telemetry_from_all_reachable_mounts() {
        let fixture = Fixture::new("/job", "/");
        fixture.write_cgroup(CgroupSettings::new("job", "100", "80", "20"));
        fixture.write_pressure(PressureSettings::new("job", 1.0, 2.0));
        let second_mount = fixture.root.join("second-cgroup");
        fixture.write_cgroup_at(&second_mount, CgroupSettings::new("", "50", "40", "10"));
        fixture.write_pressure_at(&second_mount, PressureSettings::new("", 3.0, 4.0));
        fixture.set_mountinfo(&format!(
            "42 1 0:42 / {} rw - cgroup2 cgroup rw\n43 1 0:43 /job {} rw - cgroup2 cgroup rw\n",
            fixture.mount_point.display(),
            second_mount.display()
        ));

        let sample = sample_from_paths(&fixture.proc_root).unwrap();
        assert_eq!(sample.total_bytes, 50);
        assert_eq!(sample.available_bytes, 30);
        assert_eq!(sample.full_avg10, 3.0);
        assert_eq!(sample.some_avg10, 4.0);
    }

    #[test]
    fn sample_uses_host_values_when_current_cgroup_is_root_without_controller_files() {
        let fixture = Fixture::new("/", "/");
        let sample = sample_from_paths(&fixture.proc_root).unwrap();
        assert_eq!(sample.total_bytes, 2000 * 1024);
        assert_eq!(sample.available_bytes, 1000 * 1024);
    }

    #[test]
    fn sample_skips_unreachable_cgroup_mount_before_matching_mount() {
        let fixture = Fixture::new("/tenant/job", "/tenant");
        fixture.write_cgroup(CgroupSettings::new("job", "max", "max", "100"));
        let decoy = fixture.root.join("decoy-cgroup");
        fs::create_dir_all(&decoy).unwrap();
        fixture.set_mountinfo(&format!(
            "42 1 0:42 / {} rw - cgroup2 cgroup rw\n43 1 0:43 /tenant {} rw - cgroup2 cgroup rw\n",
            decoy.display(),
            fixture.mount_point.display()
        ));

        let sample = sample_from_paths(&fixture.proc_root).unwrap();
        assert_eq!(sample.available_bytes, 1000 * 1024);
    }

    #[test]
    fn sample_allows_unlimited_limits_and_missing_psi_files() {
        let fixture = Fixture::new("/job", "/");
        fixture.write_cgroup(CgroupSettings::new("job", "max", "max", "100"));
        fs::remove_file(fixture.proc_root.join("pressure/memory")).unwrap();
        let sample = sample_from_paths(&fixture.proc_root).unwrap();
        assert_eq!(sample.full_avg10, 0.0);
        assert_eq!(sample.some_avg10, 0.0);
    }

    #[test]
    fn sample_uses_host_values_when_only_cgroup_v1_is_present() {
        let fixture = Fixture::new("/job", "/");
        fs::write(fixture.proc_root.join("self/cgroup"), "7:memory:/job\n").unwrap();
        let sample = sample_from_paths(&fixture.proc_root).unwrap();
        assert_eq!(sample.total_bytes, 2000 * 1024);
        assert_eq!(sample.available_bytes, 1000 * 1024);
        assert_eq!(sample.full_avg10, 0.25);
        assert_eq!(sample.some_avg10, 0.5);
    }

    #[test]
    fn sample_saturates_over_limit_and_check_rejects() {
        let fixture = Fixture::new("/job", "/");
        fixture.write_cgroup(CgroupSettings::new("job", "10", "5", "11"));
        let sample = sample_from_paths(&fixture.proc_root).unwrap();
        assert_eq!(sample.available_bytes, 0);
        assert!(sample.check(1).is_err());
    }

    #[test]
    fn sample_fails_closed_when_nonroot_counter_is_missing() {
        let fixture = Fixture::new("/job", "/");
        fixture.write_cgroup(CgroupSettings::new("job", "10", "5", "1"));
        fixture.remove_cgroup_file("job", "memory.current");
        assert!(sample_from_paths(&fixture.proc_root).is_err());
    }

    #[test]
    fn sample_rejects_cgroup_path_traversal() {
        let fixture = Fixture::new("/../outside", "/");
        assert_invalid(sample_from_paths(&fixture.proc_root));
        assert!(!Path::new(&fixture.root).join("outside/memory.max").exists());
    }

    #[test]
    fn check_rejects_memory_and_pressure_thresholds() {
        let healthy = MemorySample {
            total_bytes: 100,
            available_bytes: 50,
            full_avg10: 0.99,
            some_avg10: 9.99,
        };
        assert!(healthy.check(50).is_ok());
        assert!(
            healthy
                .check(51)
                .unwrap_err()
                .to_string()
                .contains("workload resource guard:")
        );

        let full = MemorySample {
            full_avg10: 1.0,
            ..healthy
        };
        assert!(
            full.check(0)
                .unwrap_err()
                .to_string()
                .contains("close other workloads")
        );

        let some = MemorySample {
            some_avg10: 10.0,
            ..healthy
        };
        assert!(some.check(0).is_err());
    }
}
