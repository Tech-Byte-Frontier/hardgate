use super::*;
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "hardgate-init-private-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn filesystem_error_paths_and_existing_entries_are_reported() {
    let invalid = PathBuf::from("\0");
    assert!(entry_exists(&invalid).is_err());
    assert!(write_new(&invalid, b"content").is_err());

    let root = scratch("write-new");
    let existing = root.join("existing.toml");
    fs::write(&existing, "sentinel = true\n").unwrap();
    assert!(!write_new(&existing, b"replacement").unwrap());
    assert_eq!(fs::read_to_string(&existing).unwrap(), "sentinel = true\n");
    assert!(entry_exists(&existing).unwrap());

    let absent = root.join("absent.toml");
    assert!(!entry_exists(&absent).unwrap());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn coverage_setup_helpers_cover_present_and_missing_reports() {
    let root = scratch("evidence");
    let mut config = Preset::StrictAgent.to_default_config();

    let mut missing = Vec::new();
    append_coverage_setup(&mut missing, &root, &config);
    assert!(missing.iter().any(|item| item.contains("coverage report")));

    fs::create_dir_all(root.join("coverage")).unwrap();
    fs::write(root.join("coverage/lcov.info"), "SF:src/lib.rs\n").unwrap();
    missing.clear();
    append_coverage_setup(&mut missing, &root, &config);
    assert!(missing.is_empty());

    config.coverage.report = None;
    append_coverage_setup(&mut missing, &root, &config);
    assert!(
        missing
            .iter()
            .any(|item| item.contains("configure coverage.report"))
    );

    config.coverage.enabled = false;
    missing.clear();
    append_coverage_setup(&mut missing, &root, &config);
    assert!(missing.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn mutation_setup_helpers_cover_empty_missing_and_present_reports() {
    let root = scratch("mutation");
    let mut config = Preset::StrictAgent.to_default_config();
    let mut missing = Vec::new();

    config.mutation.reports = None;
    append_mutation_setup(&mut missing, &root, &config);
    assert!(
        missing
            .iter()
            .any(|item| item.contains("configure mutation.reports"))
    );

    config.mutation.reports = Some(Vec::new());
    missing.clear();
    append_mutation_setup(&mut missing, &root, &config);
    assert!(
        missing
            .iter()
            .any(|item| item.contains("configure mutation.reports"))
    );

    fs::write(root.join("mutation.json"), "{}\n").unwrap();
    config.mutation.reports = Some(vec![
        "mutation.json".to_string(),
        "missing.json".to_string(),
    ]);
    missing.clear();
    append_mutation_setup(&mut missing, &root, &config);
    assert_eq!(
        missing,
        vec!["mutation report missing.json is not present yet".to_string()]
    );

    config.mutation.enabled = false;
    missing.clear();
    append_mutation_setup(&mut missing, &root, &config);
    assert!(missing.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn summary_helpers_cover_all_reference_and_engine_choices() {
    assert_eq!(parse_preset("strict-agent"), (Preset::StrictAgent, false));
    assert_eq!(parse_preset("BALANCED"), (Preset::Balanced, false));
    assert_eq!(
        parse_preset("legacy-migration"),
        (Preset::LegacyMigration, false)
    );
    assert_eq!(parse_preset("custom"), (Preset::Custom, false));
    assert_eq!(parse_preset("future"), (Preset::StrictAgent, true));

    assert_eq!(
        reference_label(ReferenceStatus::NotApplicable),
        "not checked"
    );
    assert_eq!(reference_label(ReferenceStatus::Available), "available");
    assert_eq!(reference_label(ReferenceStatus::Missing), "missing");
    assert_eq!(reference_label(ReferenceStatus::Unknown), "unknown");

    let mut config = Preset::StrictAgent.to_default_config();
    config.orchestration.format_check = Some("format --check".to_string());
    config.orchestration.format = Some("format".to_string());
    config.orchestration.lint = Some("lint".to_string());
    config.orchestration.test_cmd = Some("test".to_string());
    let engines = enabled_engines(&config);
    for engine in [
        "coverage evidence",
        "mutation evidence",
        "formatter",
        "linter",
        "tests",
    ] {
        assert!(engines.contains(engine), "missing {engine}: {engines}");
    }

    assert_eq!(
        next_command(
            Preset::LegacyMigration,
            &config,
            &[],
            ReferenceStatus::Unknown,
        ),
        "git fetch origin main"
    );
    assert_eq!(
        next_command(
            Preset::StrictAgent,
            &config,
            &["mutation report missing.json".to_string()],
            ReferenceStatus::NotApplicable,
        ),
        "hardgate config"
    );

    config.coverage.enabled = false;
    config.mutation.enabled = false;
    config.orchestration = Default::default();
    assert_eq!(
        next_command(
            Preset::Balanced,
            &config,
            &[],
            ReferenceStatus::NotApplicable,
        ),
        "hardgate check"
    );
}

#[test]
fn override_filter_and_deduplication_are_exact() {
    let mut config = Preset::Balanced.to_default_config();
    config.orchestration.format = Some("format".to_string());
    config.orchestration.lint = Some("lint".to_string());
    for message in [
        "formatter command is not configured",
        "Prettier is missing",
        "Biome is missing",
    ] {
        assert!(resolved_by_override(message, &config));
    }
    for message in [
        "linter command is not configured",
        "ESLint is missing",
        "Oxlint is missing",
    ] {
        assert!(resolved_by_override(message, &config));
    }
    assert!(!resolved_by_override(
        "test command is not configured",
        &config
    ));

    let values = deduplicate(vec![
        "one".to_string(),
        "one".to_string(),
        "two".to_string(),
        "one".to_string(),
    ]);
    assert_eq!(values, vec!["one".to_string(), "two".to_string()]);
}
