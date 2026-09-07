#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, json, run};
use serde_json::Value;

const POLICY: &str = "[gate]\npreset='custom'\n[budgets.functions]\nmax_parameters=1\n[orchestration]\nlint='sh -c true'\n";

fn fixture(tag: &str) -> Fixture {
    let fixture = Fixture::new("execution-contract", tag, Some(POLICY));
    fixture.write("src/value.rs", "pub fn value() -> bool { true }\n");
    fixture
}

fn engine<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["execution"]["engines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|engine| engine["id"] == id)
        .unwrap()
}

fn state<'a>(report: &'a Value, id: &str) -> &'a str {
    engine(report, id)["state"].as_str().unwrap()
}

#[test]
fn reports_actual_engine_states_and_separate_command_scope() {
    let fixture = fixture("states");
    let command = "check";
    let output = run(&fixture, &[command, "--checks", "policy", "--json"]);
    cli::assert_status(&output, true, command);
    let report = json(&output);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["command"], command);
    assert_eq!(report["status"], "passed");
    assert_eq!(report["exit_code"], 0);
    assert_eq!(state(&report, "complexity"), "completed");
    assert_eq!(state(&report, "coverage"), "disabled");
    assert_eq!(state(&report, "lint"), "skipped");
    assert_eq!(state(&report, "clones"), "completed");
    let report = json(&run(&fixture, &["check", "--checks", "lint", "--json"]));
    assert_eq!(state(&report, "lint"), "completed");
    let scan = json(&run(&fixture, &["scan", "src/value.rs", "--json"]));
    assert_eq!(scan["execution"]["scope"]["mode"], "paths");
    assert_eq!(state(&scan, "lint"), "skipped");
}

#[test]
fn failure_incompletion_and_empty_input_are_different_states() {
    let fixture = fixture("failures");
    fixture.write(
        "src/value.rs",
        "pub fn value(a: i32, b: i32) -> i32 { a + b }\n",
    );
    let violated = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    assert_eq!(violated["status"], "violations");
    assert_eq!(state(&violated, "complexity"), "failed");
    fixture.write(
        "hardgate.toml",
        &format!("{POLICY}\n[coverage]\nenabled=true\n"),
    );
    let incomplete = json(&run(
        &fixture,
        &["check", "--checks", "policy", "--json", "--summary"],
    ));
    assert_eq!(incomplete["exit_code"], 2);
    assert_eq!(incomplete["status"], "incomplete");
    assert_eq!(state(&incomplete, "coverage"), "incomplete");
    assert_eq!(state(&incomplete, "complexity"), "failed");
    fixture.write("hardgate.toml", POLICY);
    std::fs::create_dir_all(fixture.join("empty")).unwrap();
    let empty = json(&run(
        &fixture,
        &["check", "--checks", "policy", "empty", "--json"],
    ));
    assert_eq!(state(&empty, "complexity"), "skipped");
    assert_eq!(empty["files_scanned"], 0);
}

#[test]
fn warning_mode_retains_incomplete_analysis_without_rewriting_policy_verdict() {
    let fixture = fixture("warning");
    fixture.write("hardgate.toml", "[gate]\npreset='custom'\nstrict=false\n");
    fixture.write("src/value.rs", "pub fn value( { broken\n");
    let report = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    assert_eq!(report["passed"], true);
    assert_eq!(state(&report, "complexity"), "incomplete");
    assert!(!report["advisories"].as_array().unwrap().is_empty());
}

#[test]
fn policy_identity_tracks_effective_overrides_and_is_stable_across_scope() {
    let fixture = fixture("identity");
    let first = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    let nested = json(&run(
        &fixture.join("src"),
        &["check", "--checks", "policy", "--json"],
    ));
    let id = &first["execution"]["config"];
    assert_eq!(id, &nested["execution"]["config"]);
    assert_eq!(id["policy_sha256"].as_str().unwrap().len(), 64);
    fixture.write("hardgate.toml", &format!("# extra whitespace\n{POLICY}\n"));
    let equivalent = json(&run(&fixture, &["config", "--format", "json"]));
    assert_eq!(id, &equivalent["config_identity"]);
    fixture.write(
        "hardgate.toml",
        &POLICY.replace("max_parameters=1", "max_parameters=2"),
    );
    let changed = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    assert_ne!(
        id["policy_sha256"],
        changed["execution"]["config"]["policy_sha256"]
    );
}

#[test]
fn completions_cover_commands_and_bypass_invalid_configuration() {
    let fixture = fixture("completions");
    fixture.write("hardgate.toml", "invalid policy");
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let output = run(&fixture, &["completions", shell]);
        cli::assert_status(&output, true, shell);
        let script = cli::stdout(&output);
        assert!(script.contains("hardgate"), "{shell}");
        assert!(script.contains("threads"), "{shell}");
        assert!(script.contains("evidence"), "{shell}");
        assert!(cli::stderr(&output).is_empty());
    }
}

#[test]
fn aborts_retain_policy_and_intended_scope_without_claiming_completed_engines() {
    let fixture = fixture("aborted");
    let output = run(&fixture, &["scan", "absent.rs", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    let report = json(&output);
    assert_eq!(report["command"], "scan");
    assert_eq!(
        report["execution"]["config"]["root"],
        fixture.canonicalize().unwrap().to_string_lossy().as_ref()
    );
    assert_eq!(state(&report, "complexity"), "incomplete");
    assert_eq!(state(&report, "lint"), "skipped");
    assert!(
        report["message"]
            .as_str()
            .unwrap()
            .contains("File not found")
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn ignored_parse_failures_keep_engine_reasons_without_findings() {
    let fixture = fixture("ignored-reason");
    fixture.write(
        "hardgate.toml",
        "[gate]\npreset='custom'\n[roles.source]\nseverity='ignore'\n",
    );
    fixture.write("src/value.rs", "pub fn broken( { invalid\n");
    let report = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    assert_eq!(report["passed"], true);
    assert_eq!(state(&report, "complexity"), "incomplete");
    assert!(
        engine(&report, "complexity")["reason"]
            .as_str()
            .unwrap()
            .contains("parse")
    );
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap().contains("parse"))
    );
}

#[test]
fn coverage_without_a_receipt_is_incomplete_even_without_eligible_sources() {
    let fixture = Fixture::new(
        "execution-contract",
        "empty-coverage",
        Some("[gate]\npreset='custom'\n[coverage]\nenabled=true\nreport='coverage.info'\n"),
    );
    fixture.write(
        "tests/test.js",
        "export function works() { return true; }\n",
    );
    fixture.write("coverage.info", "TN:\nSF:tests/test.js\nFN:1,works\nFNDA:1,works\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nBRF:0\nBRH:0\nend_of_record\n");
    let report = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    assert_eq!(report["exit_code"], 2);
    assert_eq!(state(&report, "coverage"), "incomplete");
    fixture.write("coverage.info", "malformed required evidence");
    let malformed = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    assert_eq!(malformed["exit_code"], 2);
    assert_eq!(state(&malformed, "coverage"), "incomplete");
}

#[test]
fn unknown_classification_marks_selected_static_engines_incomplete() {
    let fixture = Fixture::new(
        "execution-contract",
        "unknown-role",
        Some("[gate]\npreset='custom'\nenforce_classified_sources=true\n[clones]\nenabled=true\n"),
    );
    fixture.write("src/data.xyz", "unknown source data\n");
    let report = json(&run(
        &fixture,
        &["check", "--checks", "policy", "src/data.xyz", "--json"],
    ));
    assert_eq!(report["exit_code"], 2);
    for id in [
        "file_budgets",
        "suppressions",
        "complexity",
        "invariants",
        "clones",
    ] {
        assert_eq!(state(&report, id), "incomplete", "{id}");
        assert!(
            engine(&report, id)["reason"]
                .as_str()
                .unwrap()
                .contains("classify-source")
        );
    }
}

#[test]
fn completed_engines_cover_mixed_roles_without_hiding_later_parse_failure() {
    let fixture = fixture("mixed-observations");
    fixture.write(
        "fixtures/first.rs",
        "fixture data without a Rust function\n",
    );
    let limited = json(&run(
        &fixture,
        &["check", "--checks", "policy", "fixtures", "--json"],
    ));
    assert_eq!(state(&limited, "file_budgets"), "completed");
    assert_eq!(state(&limited, "complexity"), "skipped");
    assert_eq!(state(&limited, "invariants"), "skipped");

    let full = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    for id in ["file_budgets", "suppressions", "complexity", "invariants"] {
        assert_eq!(state(&full, id), "completed", "{id}");
    }
    fixture.write("src/zz_broken.rs", "pub fn broken( { invalid\n");
    let incomplete = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    assert_eq!(incomplete["exit_code"], 2);
    assert_eq!(state(&incomplete, "complexity"), "incomplete");
    assert!(
        engine(&incomplete, "complexity")["reason"]
            .as_str()
            .unwrap()
            .contains("parse")
    );
}
