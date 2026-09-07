use super::*;

#[test]
fn only_known_cache_records_are_disposable() {
    let policy = InputPolicy::new(Path::new("/project"), &HardgateConfig::default()).unwrap();
    for path in [
        ".eslintcache",
        "nested/.eslintcache",
        "pkg/__pycache__/module.pyc",
        ".ruff_cache/CACHEDIR.TAG",
        ".ruff_cache/.gitignore",
        ".ruff_cache/0.5/abcdef0123",
        ".import_linter_cache/CACHEDIR.TAG",
        ".import_linter_cache/.gitignore",
        ".import_linter_cache/graph.data.json",
        ".import_linter_cache/graph.meta.json",
        ".pytest_cache/CACHEDIR.TAG",
        ".pytest_cache/.gitignore",
        ".pytest_cache/README.md",
        ".pytest_cache/v/cache/nodeids",
        ".pytest_cache/v/cache/lastfailed",
        ".pytest_cache/v/cache/stepwise",
        ".pytest_cache/v/cache/durations",
    ] {
        assert!(policy.is_output(Path::new(path)), "{path}");
    }
    for path in [
        "",
        "source.rs",
        "module.pyc",
        "__pycache__/source.rs",
        ".ruff_cache/source.rs",
        ".ruff_cache/0.5/not-hex",
        ".ruff_cache/0.5/abcdef/input.rs",
        ".import_linter_cache/source.rs",
        ".import_linter_cache/nested/graph.data.json",
        ".pytest_cache/required.json",
        ".pytest_cache/v/other/nodeids",
        ".pytest_cache/v/cache/required",
        ".pytest_cache/v/cache/nodeids/required.rs",
    ] {
        assert!(!policy.is_output(Path::new(path)), "{path}");
    }
}

#[test]
fn report_outputs_must_be_local_normalized_and_not_required_source() {
    let root = crate::fs_tests::tempdir("input-policy");
    let local = root.join("reports/coverage.info");
    for report in [local, PathBuf::from("./reports/./coverage.info")] {
        let mut config = HardgateConfig::default();
        config.coverage.report = Some(report.to_string_lossy().into_owned());
        let policy = InputPolicy::new(&root, &config).unwrap();
        assert!(policy.is_output(Path::new("reports/coverage.info")));
        assert!(policy.is_output(Path::new("reports/coverage.info.hardgate.json")));
        assert!(!policy.is_output(Path::new("reports/required.info")));
    }
    for report in [
        root.parent().unwrap().join("outside.info"),
        PathBuf::from("../outside.info"),
        PathBuf::from("src/source.rs"),
        PathBuf::from("reports/coverage.json"),
    ] {
        let mut config = HardgateConfig::default();
        config.coverage.report = Some(report.to_string_lossy().into_owned());
        assert!(!InputPolicy::new(&root, &config).unwrap().is_output(&report));
    }
    let config: HardgateConfig =
        toml::from_str("[[classification.rules]]\nglob='.pytest_cache/**'\nrole='config'\n")
            .unwrap();
    assert!(
        !InputPolicy::new(&root, &config)
            .unwrap()
            .is_output(Path::new(".pytest_cache/v/cache/nodeids"))
    );
    std::fs::remove_dir_all(root).unwrap();
}
