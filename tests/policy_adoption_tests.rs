#[path = "common/cli.rs"]
mod cli;
#[path = "common/fs_git.rs"]
mod fs_git;

use cli::{Fixture, assert_status, json, run};
use hardgate::commands::run_static_gate_snapshot;
use hardgate::config::{HardgateConfig, Preset, Severity};
use std::path::{Path, PathBuf};

#[test]
fn init_defaults_to_structural_policy_but_explicit_strict_requires_evidence() {
    let balanced = Fixture::new("policy-adoption", "default", None);
    let output = run(balanced.as_ref(), &["init"]);
    assert_status(&output, true, "default initialization");
    let config =
        HardgateConfig::load_or_default(Some(&balanced.as_ref().join("hardgate.toml"))).unwrap();
    assert_eq!(config.gate.preset, Preset::Balanced);
    assert!(!config.coverage.enabled && !config.mutation.enabled);
    balanced.write("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    let output = run(balanced.as_ref(), &["check", "--json"]);
    assert_status(&output, true, "structural adoption check");
    let report = json(&output);
    assert!(report["advisories"].as_array().unwrap().iter().any(|item| {
        item.as_str()
            .unwrap()
            .contains("coverage evidence (disabled by policy)")
    }));

    let strict = Fixture::new("policy-adoption", "strict", None);
    assert_status(
        &run(strict.as_ref(), &["init", "--preset", "strict-agent"]),
        true,
        "strict initialization",
    );
    strict.write("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    let output = run(strict.as_ref(), &["check", "--json"]);
    assert_status(&output, false, "strict missing evidence");
    let report = json(&output);
    for step in ["coverage-report", "mutation-report"] {
        assert!(
            report["orchestration_violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["step"] == step),
            "missing {step}: {report}"
        );
    }
}

#[test]
fn test_size_is_visible_while_test_complexity_and_safety_remain_blocking() {
    let mut config = Preset::StrictAgent.to_default_config();
    config.clones.enabled = false;
    config.budgets.files.max_lines.insert("rs".into(), 10);
    let statements = (0..35)
        .map(|n| format!("    let value_{n} = {n};\n"))
        .collect::<String>();
    let straight = format!("fn setup() {{\n{statements}}}\n");
    let (report, _, _, _) = run_static_gate_snapshot(&config, &[
        (PathBuf::from("tests/setup.rs"), straight.clone()),
        (PathBuf::from("src/setup.rs"), straight),
        (PathBuf::from("tests/complex.rs"),
            "fn complex(x: bool) { if x { if x { if x { if x { if x {} } } } } }\n// @ts-ignore\n".into()),
    ]).unwrap();
    assert!(
        report
            .budget_violations
            .iter()
            .all(|item| item.file.starts_with("src"))
    );
    assert!(!report.budget_violations.is_empty());
    assert!(
        report
            .advisories
            .iter()
            .any(|item| item.contains("tests/setup.rs") && item.contains("file budget"))
    );
    assert!(
        report
            .advisories
            .iter()
            .any(|item| item.contains("tests/setup.rs") && item.contains("Statement Count"))
    );
    assert!(
        report.complexity_violations.iter().any(
            |item| item.file == Path::new("tests/complex.rs") && item.metric == "Nesting Depth"
        )
    );
    assert!(
        report
            .suppression_violations
            .iter()
            .any(|item| item.file == Path::new("tests/complex.rs"))
    );
}

#[test]
fn role_specific_policy_round_trips_and_explicit_errors_restore_size_gates() {
    let fixture = Fixture::new("policy-adoption", "overrides", None);
    fixture.write(
        "hardgate.toml",
        r#"[gate]
preset = "balanced"
[roles.test]
file_size_severity = "error"
function_size_severity = "error"
clone_severity = "error"
max_lines = 1
max_statements = 1
clone_block_min_lines = 1
clone_block_min_tokens = 1
"#,
    );
    let config =
        HardgateConfig::load_or_default(Some(&fixture.as_ref().join("hardgate.toml"))).unwrap();
    assert_eq!(config.roles.test.file_size_severity, Some(Severity::Error));
    assert_eq!(
        config.roles.test.function_size_severity,
        Some(Severity::Error)
    );
    assert_eq!(config.roles.test.clone_severity, Some(Severity::Error));
    assert_eq!(config.roles.test.clone_block_min_lines, Some(1));
    assert_eq!(config.roles.source.clone_block_min_tokens, Some(150));
    let serialized = toml::to_string(&config).unwrap();
    fixture.write("roundtrip.toml", &serialized);
    let roundtrip =
        HardgateConfig::load_or_default(Some(&fixture.as_ref().join("roundtrip.toml"))).unwrap();
    assert_eq!(config.roles, roundtrip.roles);
    fixture.write(
        "tests/example.rs",
        "fn test() {\n let a = 1;\n let b = 2;\n}\n",
    );
    let output = run(fixture.as_ref(), &["check", "--json"]);
    assert_status(&output, false, "explicit test size enforcement");
    let report = json(&output);
    assert!(!report["budget_violations"].as_array().unwrap().is_empty());
    assert!(
        report["complexity_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["metric"] == "Statement Count")
    );
}

#[test]
fn small_clones_stay_visible_and_larger_clones_still_block() {
    let content = "fn calculate(x: i32) -> i32 {\n let a = x + 1;\n let b = a * 2;\n let c = b - 3;\n let d = c / 4;\n let e = d + 5;\n let f = e * 6;\n let g = f - 7;\n let h = g + 8;\n h\n}\n";
    let files = [
        (PathBuf::from("src/first.rs"), content.to_string()),
        (PathBuf::from("src/second.rs"), content.to_string()),
    ];
    let mut config = Preset::StrictAgent.to_default_config();
    let (report, _, _, _) = run_static_gate_snapshot(&config, &files).unwrap();
    assert!(report.clone_violations.is_empty());
    assert!(
        report
            .advisories
            .iter()
            .any(|item| item.contains("Detected duplication") && item.contains("fingerprint"))
    );
    config.roles.source.clone_block_min_lines = Some(1);
    config.roles.source.clone_block_min_tokens = Some(1);
    let (report, _, _, _) = run_static_gate_snapshot(&config, &files).unwrap();
    assert!(!report.clone_violations.is_empty());
}

#[test]
fn invalid_new_thresholds_fail_configuration_loading() {
    for field in ["clone_block_min_lines", "clone_block_min_tokens"] {
        let fixture = Fixture::new("policy-adoption", field, None);
        fixture.write("hardgate.toml", &format!("[roles.source]\n{field} = 0\n"));
        let error = HardgateConfig::load_or_default(Some(&fixture.as_ref().join("hardgate.toml")))
            .unwrap_err();
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn advisory_categories_do_not_hide_test_parser_failures() {
    let config = Preset::Balanced.to_default_config();
    let (report, _, _, _) = run_static_gate_snapshot(
        &config,
        &[(PathBuf::from("tests/broken.rs"), "fn broken( {".into())],
    )
    .unwrap();
    assert!(
        report
            .orchestration_violations
            .iter()
            .any(|item| item.step == "parse-source")
    );
}

#[test]
fn calibrated_numeric_boundaries_keep_complexity_and_strict_evidence() {
    use hardgate::engines::ComplexityAnalyzer;
    use std::path::Path;
    let mut analyzer = ComplexityAnalyzer::new();
    let source = "def five(a, b, *, c, d, e):\n    return a + b + c + d + e\ndef six(a, b, *, c, d, e, f):\n    return a + b + c + d + e + f\n";
    let metrics = analyzer
        .analyze_file_checked(Path::new("src/args.py"), source, Path::new("."))
        .unwrap();
    let strict = Preset::StrictAgent.to_default_config();
    let findings = ComplexityAnalyzer::check_violations(&metrics, &strict.budgets.functions);
    let parameters = findings
        .iter()
        .filter(|item| item.metric == "Parameter Count")
        .collect::<Vec<_>>();
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].function_name, "six");
    assert_eq!(strict.budgets.functions.max_cyclomatic, Some(10));
    assert_eq!(strict.budgets.functions.max_cognitive, Some(15));
    assert_eq!(strict.budgets.functions.max_nesting_depth, Some(4));
    assert!(strict.coverage.enabled && strict.mutation.enabled);
    assert_eq!(strict.coverage.min_line_percent, Some(95.0));
    assert_eq!(strict.coverage.min_branch_percent, Some(90.0));
    assert_eq!(strict.mutation.min_score, Some(85.0));

    let balanced = Preset::Balanced.to_default_config();
    for (count, should_block) in [(50, false), (51, true)] {
        let body = (0..count)
            .map(|n| format!("let x{n} = {n};\n"))
            .collect::<String>();
        let metrics = analyzer
            .analyze_file_checked(
                Path::new("src/straight.rs"),
                &format!("fn straight() {{\n{body}}}"),
                Path::new("."),
            )
            .unwrap();
        let findings = ComplexityAnalyzer::check_violations(&metrics, &balanced.budgets.functions);
        assert_eq!(
            findings.iter().any(|item| item.metric == "Statement Count"),
            should_block
        );
    }
}

#[test]
fn adoption_preserves_baseline_debt_but_blocks_new_debt_and_missing_evidence() {
    let fixture = Fixture::new("policy-adoption", "ratchet", None);
    fixture.write(
        "hardgate.toml",
        "[gate]\npreset = \"legacy-migration\"\n[legacy]\nreference_branch = \"HEAD\"\n",
    );
    fs_git::write(
        fixture.as_ref(),
        "src/lib.rs",
        "pub fn existing(a:i32,b:i32,c:i32,d:i32,e:i32,f:i32,g:i32) {}\n",
    );
    fs_git::init_repo(fixture.as_ref());
    fs_git::commit_baseline(fixture.as_ref(), "baseline");
    let output = run(fixture.as_ref(), &["check", "--json"]);
    assert_status(&output, true, "existing debt adoption");
    let report = json(&output);
    assert!(
        report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().unwrap().contains("not a debt-free"))
    );
    fixture.write(
        "src/new.rs",
        "pub fn added(a:i32,b:i32,c:i32,d:i32,e:i32,f:i32,g:i32) {}\n",
    );
    assert_status(
        &run(fixture.as_ref(), &["check", "--json"]),
        false,
        "new debt blocks",
    );
    std::fs::remove_file(fixture.as_ref().join("src/new.rs")).unwrap();
    fixture.write("hardgate.toml", "[gate]\npreset = \"legacy-migration\"\n[legacy]\nreference_branch = \"HEAD\"\n[coverage]\nenabled = true\nreport = \"missing.info\"\n");
    let output = run(fixture.as_ref(), &["check", "--json"]);
    assert_status(&output, false, "ratchet cannot waive current evidence");
    assert!(
        json(&output)["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["step"] == "coverage-report")
    );
}
