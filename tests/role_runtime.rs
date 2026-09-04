use hardgate::commands::run_static_gate_snapshot;
use hardgate::config::{HardgateConfig, Severity};
use std::path::{Path, PathBuf};

fn snapshot(config: &HardgateConfig, files: &[(&str, &str)]) -> hardgate::diagnostics::GateReport {
    let contents = files
        .iter()
        .map(|(path, content)| (PathBuf::from(path), (*content).to_string()))
        .collect::<Vec<_>>();
    run_static_gate_snapshot(config, &contents)
        .expect("snapshot static gate should classify and analyze")
        .0
}

fn branch() -> &'static str {
    "fn branch(value: bool) -> bool { if value { true } else { false } }"
}

fn clone_body(name: &str) -> String {
    format!(
        "fn {name}() {{\n    noop();\n    noop();\n    noop();\n    noop();\n    noop();\n    noop();\n    noop();\n    noop();\n}}"
    )
}

#[test]
fn ordered_custom_classification_drives_runtime_policy() {
    let mut config = HardgateConfig::default();
    config.clones.enabled = false;
    config.roles.source.max_cyclomatic = Some(100);
    config.roles.test.max_cyclomatic = Some(0);
    config.classification = toml::from_str(
        r#"
        [[rules]]
        glob = "src/**"
        role = "test"

        [[rules]]
        glob = "src/special.rs"
        role = "source"
        "#,
    )
    .unwrap();

    let report = snapshot(&config, &[("src/special.rs", branch())]);
    assert!(report.complexity_violations.iter().any(|finding| {
        finding.file.as_path() == Path::new("src/special.rs")
            && finding.metric == "Cyclomatic Complexity"
    }));
}

#[test]
fn source_and_test_clone_thresholds_are_independent() {
    let mut config = HardgateConfig::default();
    config.roles.source.clone_min_lines = Some(5);
    config.roles.source.clone_min_tokens = Some(20);
    config.roles.test.clone_min_lines = Some(20);
    config.roles.test.clone_min_tokens = Some(20);
    let source_a = clone_body("source_a");
    let source_b = clone_body("source_b");
    let test_a = clone_body("test_a");
    let test_b = clone_body("test_b");
    let report = snapshot(
        &config,
        &[
            ("src/source_a.rs", &source_a),
            ("src/source_b.rs", &source_b),
            ("tests/test_a.rs", &test_a),
            ("tests/test_b.rs", &test_b),
        ],
    );
    assert!(!report.clone_violations.is_empty());
    assert!(report.clone_violations.iter().all(|finding| {
        finding.file_a.starts_with("src/") && finding.file_b.starts_with("src/")
    }));
}

#[test]
fn fixture_warning_is_advisory_not_error() {
    let mut config = HardgateConfig::default();
    config.clones.enabled = false;
    config.roles.fixture.severity = Some(Severity::Warning);
    config.roles.fixture.max_lines = Some(1);
    let report = snapshot(
        &config,
        &[("tests/__fixtures__/state.rs", "line one\nline two\n")],
    );

    assert!(report.budget_violations.is_empty());
    assert!(
        report
            .advisories
            .iter()
            .any(|advisory| advisory.contains("file budget") && advisory.contains("state.rs"))
    );
}

#[test]
fn generated_files_are_inventoried_without_handwritten_debt() {
    let config = HardgateConfig::default();
    let generated_a = clone_body("generated_a");
    let generated_b = clone_body("generated_b");
    let report = snapshot(
        &config,
        &[
            ("src/generated/a.rs", &generated_a),
            ("src/generated/b.rs", &generated_b),
        ],
    );

    assert!(report.complexity_violations.is_empty());
    assert!(report.clone_violations.is_empty());
    assert!(
        report
            .advisories
            .iter()
            .any(|advisory| advisory.contains("generated file"))
    );
}

#[test]
fn unsupported_migration_is_error_even_when_gate_is_not_strict() {
    let mut config = HardgateConfig::default();
    config.gate.strict = false;
    config.clones.enabled = false;
    let report = snapshot(
        &config,
        &[("migrations/001_init.sql", "create table users;")],
    );

    assert!(
        report
            .orchestration_violations
            .iter()
            .any(|finding| finding.step == "unsupported-source")
    );
    assert!(report.advisories.iter().all(|advisory| {
        !advisory.contains("unsupported-source") || !advisory.contains("migration")
    }));
}

#[test]
fn snapshot_uses_role_function_policy() {
    let mut config = HardgateConfig::default();
    config.clones.enabled = false;
    config.budgets.functions.max_cyclomatic = Some(100);
    config.roles.source.max_cyclomatic = Some(0);
    let report = snapshot(&config, &[("src/snapshot.rs", branch())]);

    assert!(
        report
            .complexity_violations
            .iter()
            .any(|finding| finding.metric == "Cyclomatic Complexity")
    );
}
