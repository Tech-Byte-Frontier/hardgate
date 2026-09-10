use super::*;

#[test]
fn disposable_test_reports_cannot_refresh_or_replace_valid_stale_or_tampered_evidence() {
    let project = Project::new();
    let config = "[gate]\npreset='balanced'\n[orchestration]\nformat_check='true'\nlint='true'\ntest_cmd=\"sh -c 'mkdir -p .hardgate/evidence; printf untrusted > .hardgate/evidence/coverage.lcov'\"\n[coverage]\nenabled=true\nreport='.hardgate/evidence/coverage.lcov'\n";
    std::fs::write(project.0.join("hardgate.toml"), config).unwrap();
    assert_exit(&project.produce("vitest", lcov(), ("pass", 0)), 0);
    let evidence = std::fs::read(project.report("coverage")).unwrap();
    let receipt = std::fs::read(project.receipt("coverage")).unwrap();
    let output_dir = project.0.join("coverage");
    std::fs::create_dir(&output_dir).unwrap();
    let report = project.report("coverage");
    let receipt_path = project.receipt("coverage");
    let check = || {
        let output = project
            .command()
            .args(["check", "src/lib.rs", "--json"])
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            !value["orchestration_violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["step"] == "test"),
            "{value}"
        );
        value
    };
    let valid = check();
    assert_eq!(valid["summary"]["analysis_blockers"], 0, "{valid}");
    assert_eq!(std::fs::read(&report).unwrap(), evidence);
    assert_eq!(std::fs::read(&receipt_path).unwrap(), receipt);
    std::fs::write(
        project.0.join("src/lib.rs"),
        "pub fn changed() -> u32 { 42 }\n",
    )
    .unwrap();
    let stale = check();
    assert_eq!(stale["accepted"], false);
    assert!(stale.to_string().contains("stale evidence"));
    std::fs::write(project.0.join("src/lib.rs"), SOURCE).unwrap();
    std::fs::write(&report, "tampered").unwrap();
    let tampered = check();
    assert_eq!(tampered["accepted"], false);
    assert!(tampered.to_string().contains("report bytes changed"));
    assert_eq!(std::fs::read_to_string(&report).unwrap(), "tampered");
    assert_eq!(
        std::fs::read_to_string(project.0.join("hardgate.toml")).unwrap(),
        config
    );
}
