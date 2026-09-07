#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, assert_status, json, run};

const POLICY: &str = "[gate]\npreset = 'custom'\n";

#[test]
fn static_analysis_and_saved_reports_need_no_project_tools() {
    let fixture = Fixture::new("portable", "static", Some(POLICY));
    fixture.write("src/lib.rs", "pub fn answer() -> u32 { 42 }\n");
    assert_status(
        &run(&fixture, &["scan", "src/lib.rs", "--json"]),
        true,
        "scan",
    );
    let checked = run(
        &fixture,
        &[
            "check",
            "--checks",
            "policy",
            "--json",
            "--report-json",
            "gate.json",
        ],
    );
    assert_status(&checked, true, "policy check");
    assert_eq!(json(&checked)["partial"], true);
    assert_eq!(json(&checked)["accepted"], false);
    assert_status(
        &run(&fixture, &["report", "gate.json", "--json"]),
        true,
        "saved report",
    );
    assert_status(
        &run(
            &fixture,
            &["report", "compare", "gate.json", "gate.json", "--json"],
        ),
        true,
        "report comparison",
    );
}

#[test]
fn disabled_freshness_does_not_require_isolation() {
    let fixture = Fixture::new("portable", "disabled", Some(POLICY));
    fixture.write("src/lib.rs", "pub fn answer() -> u32 { 42 }\n");
    fixture.write(
        "hardgate.toml",
        &format!(
            "{POLICY}[generated]\nenabled = false\nfreshness_command = 'missing-project-tool'\n"
        ),
    );
    assert_status(
        &run(&fixture, &["check", "--checks", "policy", "--json"]),
        true,
        "disabled freshness",
    );
}

fn tool_fixture(tag: &str, commands: &str) -> Fixture {
    let fixture = Fixture::new(
        "portable",
        tag,
        Some(&format!("{POLICY}[orchestration]\n{commands}")),
    );
    fixture.write("src/lib.rs", "pub fn answer() -> u32 { 42 }\n");
    fixture
}

#[test]
fn complete_native_check_runs_every_configured_tool() {
    let fixture = tool_fixture(
        "full",
        "format_check = 'sh format.sh'\nlint = 'sh lint.sh'\ntest_cmd = 'sh test.sh'\ntypecheck = 'sh typecheck.sh'\n",
    );
    for tool in ["format", "lint", "test", "typecheck"] {
        fixture.write(
            &format!("{tool}.sh"),
            &format!("test -f src/lib.rs || exit 7\necho ran-{tool}\n"),
        );
    }
    let output = run(&fixture, &["check", "--json"]);
    assert_status(&output, true, "complete native check");
    let report = json(&output);
    assert_eq!(report["partial"], false);
    assert_eq!(report["accepted"], true);
    for id in ["format_check", "lint", "tests", "typecheck"] {
        let engines = report["execution"]["engines"].as_array().unwrap();
        let engine = engines.iter().find(|engine| engine["id"] == id).unwrap();
        assert_eq!(engine["state"], "completed");
    }
    #[cfg(target_os = "macos")]
    assert!(
        report["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value
                .as_str()
                .unwrap()
                .contains("isolation are not enforced"))
    );
}

#[test]
fn native_failures_and_input_writes_cannot_pass() {
    let fixture = tool_fixture(
        "failures",
        "format_check = 'sh check.sh'\nlint = 'sh check.sh'\n",
    );
    fixture.write("check.sh", "echo tool-failed; exit 7\n");
    let failed = run(&fixture, &["check", "--json"]);
    assert_status(&failed, false, "failed tool");
    assert_eq!(json(&failed)["accepted"], false);
    assert!(cli::stdout(&failed).contains("tool-failed"));
    fixture.write("check.sh", "echo changed > src/lib.rs\n");
    let changed = run(&fixture, &["check", "--json"]);
    assert_status(&changed, false, "input write");
    assert_eq!(json(&changed)["accepted"], false);
    assert!(cli::stdout(&changed).contains("wrote project inputs"));
    assert_eq!(
        std::fs::read_to_string(fixture.join("src/lib.rs")).unwrap(),
        "pub fn answer() -> u32 { 42 }\n"
    );
}

#[test]
fn native_fmt_and_generated_freshness_execute() {
    let fixture = tool_fixture("fmt", "format = 'sh format.sh'\n");
    fixture.write("format.sh", "echo 'pub fn formatted() {}' > src/lib.rs\n");
    assert_status(&run(&fixture, &["fmt"]), true, "native formatting");
    assert_eq!(
        std::fs::read_to_string(fixture.join("src/lib.rs")).unwrap(),
        "pub fn formatted() {}\n"
    );
    fixture.write(
        "hardgate.toml",
        &format!("{POLICY}[generated]\nenabled = true\nfreshness_command = 'sh fresh.sh'\n"),
    );
    fixture.write("fresh.sh", "test -f src/lib.rs\n");
    assert_status(
        &run(&fixture, &["check", "--checks", "policy", "--json"]),
        true,
        "native freshness",
    );
}

#[test]
fn native_timeout_remains_incomplete() {
    let fixture = tool_fixture(
        "timeout",
        "format_check = 'sh slow.sh'\nlint = 'sh slow.sh'\ntimeout_secs = 1\n",
    );
    fixture.write("slow.sh", "sleep 20\n");
    let output = run(&fixture, &["check", "--checks", "format", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["accepted"], false);
    assert!(cli::stdout(&output).contains("timed out"));
}

#[cfg(not(target_os = "linux"))]
#[test]
fn required_isolation_and_evidence_fail_before_tools_start() {
    let fixture = tool_fixture(
        "required",
        "require_isolation = true\nformat_check = 'sh marker.sh'\nlint = 'sh marker.sh'\nformat = 'sh marker.sh'\n",
    );
    fixture.write("marker.sh", "touch started\n");
    for args in [
        vec!["check", "--json"],
        vec!["fmt"],
        vec!["evidence", "vitest"],
    ] {
        let output = run(&fixture, &args);
        assert_eq!(output.status.code(), Some(2));
        assert!(
            format!("{}{}", cli::stdout(&output), cli::stderr(&output))
                .contains("requires Linux cgroup v2")
        );
        assert!(!fixture.join("started").exists());
    }
    fixture.write("hardgate.toml", POLICY);
    let output = run(&fixture, &["evidence", "vitest"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(cli::stderr(&output).contains("requires Linux cgroup v2"));
}

#[test]
fn isolation_policy_survives_preset_merging() {
    let fixture = Fixture::new(
        "portable",
        "merged-isolation",
        Some("[gate]\npreset = 'balanced'\n[orchestration]\nrequire_isolation = true\n"),
    );
    let context =
        hardgate::config::ConfigContext::load(Some(&fixture.join("hardgate.toml"))).unwrap();
    assert!(context.config.orchestration.require_isolation);
}

#[cfg(target_os = "macos")]
#[test]
fn library_callers_cannot_bypass_required_isolation() {
    let fixture = tool_fixture(
        "library",
        "require_isolation = true\nformat = 'sh marker.sh'\n",
    );
    fixture.write("marker.sh", "touch started\n");
    let context =
        hardgate::config::ConfigContext::load(Some(&fixture.join("hardgate.toml"))).unwrap();
    let engine = hardgate::engines::OrchestrationEngine::new(&context.config.orchestration);
    let error = engine.run_format(&fixture).unwrap().unwrap_err();
    assert!(error.output.contains("requires verified Linux"));
    assert!(!fixture.join("started").exists());
}
