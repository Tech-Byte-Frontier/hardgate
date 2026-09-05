use super::*;

#[test]
fn worker_defaults_preserve_headroom_and_smaller_explicit_limits() {
    assert_eq!(select_workers(None, None, 2).unwrap(), 2);
    assert_eq!(select_workers(None, Some(64), 2).unwrap(), 2);
    assert_eq!(select_workers(None, Some(1), 2).unwrap(), 1);
    assert_eq!(select_workers(None, Some(0), 1).unwrap(), 1);
    assert_eq!(select_workers(Some(1), Some(64), 2).unwrap(), 1);
    assert!(select_workers(Some(0), None, 2).is_err());
    assert!(select_workers(Some(3), None, 2).is_err());
}

#[test]
fn external_worker_settings_are_capped_and_jobserver_is_removed() {
    let mut command = Command::new("unused-test-command");
    constrain_command(&mut command);
    let environment = command
        .get_envs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        environment.get(std::ffi::OsStr::new("CARGO_MAKEFLAGS")),
        Some(&None)
    );
    for name in [
        "CARGO_BUILD_JOBS",
        "RUST_TEST_THREADS",
        "RAYON_NUM_THREADS",
        "GOMAXPROCS",
    ] {
        let value = environment[std::ffi::OsStr::new(name)]
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert!((1..=2).contains(&value));
    }
}
