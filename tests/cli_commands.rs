#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, assert_status, json, run, stderr as stderr_text, stdout as output_text};
use serde_json::Value;

const CUSTOM_CONFIG: &str = r#"[gate]
preset = "custom"
strict = true

[coverage]
enabled = false

[mutation]
enabled = false
"#;

#[test]
fn fmt_check_runs_configured_check_command() {
    let fixture = Fixture::new("cli-commands", "fmt-check", None);
    fixture.write(
        "hardgate.toml",
        r#"[gate]
preset = "custom"

[orchestration]
format_check = "sh -c 'printf fmt-check-ok'"
timeout_secs = 2
"#,
    );

    let output = run(fixture.as_ref(), &["fmt", "--check"]);
    assert_status(&output, true, "fmt --check");
    let stdout = output_text(&output);
    assert!(stdout.contains("fmt-check-ok"));
    assert!(stdout.contains("format [sh -c 'printf fmt-check-ok'] passed"));
}

#[test]
fn fmt_runs_write_command_and_falls_back_to_check_command() {
    let fixture = Fixture::new("cli-commands", "fmt-success", None);
    fixture.write(
        "hardgate.toml",
        r#"[gate]
preset = "custom"

[orchestration]
format = "sh -c 'printf formatted > formatted.txt'"
timeout_secs = 2
"#,
    );

    let output = run(fixture.as_ref(), &["fmt"]);
    assert_status(&output, true, "fmt");
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("formatted.txt")).unwrap(),
        "formatted"
    );
    assert!(
        output_text(&output).contains("format [sh -c 'printf formatted > formatted.txt'] passed")
    );

    let fallback = Fixture::new("cli-commands", "fmt-fallback", None);
    fallback.write(
        "hardgate.toml",
        r#"[gate]
preset = "custom"

[orchestration]
format_check = "sh -c 'printf fallback-ok'"
timeout_secs = 2
"#,
    );
    let output = run(fallback.as_ref(), &["fmt"]);
    assert_status(&output, true, "fmt fallback");
    assert!(output_text(&output).contains("fallback-ok"));
}

#[test]
fn fmt_without_command_warns_and_exits_successfully() {
    let fixture = Fixture::new("cli-commands", "fmt-none", None);
    fixture.write("hardgate.toml", CUSTOM_CONFIG);

    let output = run(fixture.as_ref(), &["fmt", "--check"]);
    assert_status(&output, true, "fmt --check without command");
    assert!(output_text(&output).contains("no format or format_check command configured"));
}

#[test]
fn fmt_failure_reports_command_output_and_exits_nonzero() {
    let fixture = Fixture::new("cli-commands", "fmt-failure", None);
    fixture.write(
        "hardgate.toml",
        r#"[gate]
preset = "custom"

[orchestration]
format_check = "sh -c 'printf fmt-bad >&2; exit 7'"
timeout_secs = 2
"#,
    );

    let output = run(fixture.as_ref(), &["fmt", "--check"]);
    assert!(
        !output.status.success(),
        "a failing formatter must fail fmt"
    );
    let stderr = stderr_text(&output);
    assert!(stderr.contains("format [sh -c 'printf fmt-bad >&2; exit 7'] failed"));
    assert!(stderr.contains("fmt-bad"));
}

#[test]
fn init_uses_strict_agent_by_default_and_supports_each_preset() {
    let cases = [
        ("default", None, "strict-agent", "StrictAgent"),
        (
            "strict",
            Some("strict-agent"),
            "strict-agent",
            "StrictAgent",
        ),
        ("balanced", Some("balanced"), "balanced", "Balanced"),
        (
            "legacy",
            Some("legacy-migration"),
            "legacy-migration",
            "LegacyMigration",
        ),
        ("custom", Some("custom"), "custom", "Custom"),
    ];

    for (tag, preset, config_value, debug_name) in cases {
        let fixture = Fixture::new("cli-commands", &format!("init-{tag}"), None);
        let args = preset.map_or_else(|| vec!["init"], |value| vec!["init", "--preset", value]);
        let output = run(fixture.as_ref(), &args);
        assert_status(&output, true, &format!("init {tag}"));
        let config = std::fs::read_to_string(fixture.0.join("hardgate.toml")).unwrap();
        assert!(config.contains(&format!("preset = \"{config_value}\"")));
        assert!(output_text(&output).contains(&format!("preset [{debug_name}]")));
    }
}

#[test]
fn init_never_overwrites_an_existing_config() {
    let fixture = Fixture::new("cli-commands", "init-no-overwrite", None);
    let original = "[gate]\nname = \"keep-me\"\n";
    fixture.write("hardgate.toml", original);

    let output = run(fixture.as_ref(), &["init", "--preset", "balanced"]);
    assert_status(&output, true, "init with existing config");
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("hardgate.toml")).unwrap(),
        original
    );
    assert!(output_text(&output).contains("already exists in this directory"));
}

#[test]
fn empty_check_and_verify_render_every_output_mode() {
    let fixture = Fixture::new("cli-commands", "empty-gates", None);
    fixture.write("hardgate.toml", CUSTOM_CONFIG);

    for command in ["check", "verify"] {
        let json_output = run(fixture.as_ref(), &[command, "--format", "json"]);
        assert_status(&json_output, true, &format!("{command} json"));
        let report = json(&json_output);
        assert_eq!(report["passed"], true);
        assert!(report["advisories"].as_array().is_some());

        let agent = run(fixture.as_ref(), &[command, "--format", "agent"]);
        assert_status(&agent, true, &format!("{command} agent"));
        assert!(output_text(&agent).contains("Hardgate Passed"));

        let compact = run(fixture.as_ref(), &[command, "--format", "compact"]);
        assert_status(&compact, true, &format!("{command} compact"));
        assert!(output_text(&compact).contains("result: pass"));

        let summary = run(fixture.as_ref(), &[command, "--format", "summary"]);
        assert_status(&summary, true, &format!("{command} summary"));
        assert!(output_text(&summary).contains("Summary: 0 errors"));
    }
}

#[test]
fn scan_missing_file_returns_structured_json_error() {
    let fixture = Fixture::new("cli-commands", "scan-missing", None);
    fixture.write("hardgate.toml", CUSTOM_CONFIG);

    let output = run(
        fixture.as_ref(),
        &["scan", "missing.rs", "--format", "json"],
    );
    assert!(!output.status.success());
    let failure: Value = json(&output);
    assert_eq!(failure["passed"], false);
    assert_eq!(failure["stage"], "scan");
    assert_eq!(failure["kind"], "command-error");
    assert!(
        failure["message"]
            .as_str()
            .unwrap()
            .contains("File not found")
    );
}
