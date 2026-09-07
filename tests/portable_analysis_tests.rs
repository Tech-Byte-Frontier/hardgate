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

#[cfg(not(target_os = "linux"))]
#[test]
fn execution_features_fail_before_starting_project_tools() {
    let fixture = Fixture::new("portable", "execution", Some(POLICY));
    fixture.write("src/lib.rs", "pub fn answer() -> u32 { 42 }\n");
    fixture.write(
        "hardgate.toml",
        &format!(
            "{POLICY}[generated]\nenabled = true\nfreshness_command = 'missing-project-tool'\n"
        ),
    );
    for args in [
        vec!["check", "--checks", "policy", "--json"],
        vec!["check", "--checks", "tests", "--json"],
    ] {
        let result = run(&fixture, &args);
        assert_eq!(result.status.code(), Some(2));
        assert!(
            json(&result)["message"]
                .as_str()
                .unwrap()
                .contains("requires Linux cgroup v2")
        );
    }
    // Static scan stays available even when policy enables external freshness.
    assert_status(
        &run(&fixture, &["scan", "src/lib.rs", "--json"]),
        true,
        "scan with freshness policy",
    );
}
