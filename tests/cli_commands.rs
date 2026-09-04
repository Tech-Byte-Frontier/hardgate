#[path = "common/cli.rs"]
mod cli;
#[path = "support/fs.rs"]
mod fs;

use cli::{json, run};
use fs::tempdir;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Output;

const CUSTOM_CONFIG: &str = r#"[gate]
preset = "custom"
strict = true

[coverage]
enabled = false

[mutation]
enabled = false
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        Self(tempdir(&format!("cli-commands-{tag}")))
    }

    fn write(&self, path: &str, content: &str) {
        let target = self.0.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, content).unwrap();
    }
}

impl AsRef<Path> for Fixture {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn output_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stdout={} stderr={}",
        output_text(output),
        stderr_text(output)
    );
}

#[test]
fn fmt_check_runs_configured_check_command() {
    let fixture = Fixture::new("fmt-check");
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
    assert_success(&output, "fmt --check");
    let stdout = output_text(&output);
    assert!(stdout.contains("fmt-check-ok"));
    assert!(stdout.contains("format [sh -c 'printf fmt-check-ok'] passed"));
}

#[test]
fn fmt_runs_write_command_and_falls_back_to_check_command() {
    let fixture = Fixture::new("fmt-success");
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
    assert_success(&output, "fmt");
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("formatted.txt")).unwrap(),
        "formatted"
    );
    assert!(
        output_text(&output).contains("format [sh -c 'printf formatted > formatted.txt'] passed")
    );

    let fallback = Fixture::new("fmt-fallback");
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
    assert_success(&output, "fmt fallback");
    assert!(output_text(&output).contains("fallback-ok"));
}

#[test]
fn fmt_without_command_warns_and_exits_successfully() {
    let fixture = Fixture::new("fmt-none");
    fixture.write("hardgate.toml", CUSTOM_CONFIG);

    let output = run(fixture.as_ref(), &["fmt", "--check"]);
    assert_success(&output, "fmt --check without command");
    assert!(output_text(&output).contains("no format or format_check command configured"));
}

#[test]
fn fmt_failure_reports_command_output_and_exits_nonzero() {
    let fixture = Fixture::new("fmt-failure");
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
        let fixture = Fixture::new(&format!("init-{tag}"));
        let args = preset.map_or_else(|| vec!["init"], |value| vec!["init", "--preset", value]);
        let output = run(fixture.as_ref(), &args);
        assert_success(&output, &format!("init {tag}"));
        let config = std::fs::read_to_string(fixture.0.join("hardgate.toml")).unwrap();
        assert!(config.contains(&format!("preset = \"{config_value}\"")));
        assert!(output_text(&output).contains(&format!("preset [{debug_name}]")));
    }
}

#[test]
fn init_never_overwrites_an_existing_config() {
    let fixture = Fixture::new("init-no-overwrite");
    let original = "[gate]\nname = \"keep-me\"\n";
    fixture.write("hardgate.toml", original);

    let output = run(fixture.as_ref(), &["init", "--preset", "balanced"]);
    assert_success(&output, "init with existing config");
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("hardgate.toml")).unwrap(),
        original
    );
    assert!(output_text(&output).contains("already exists in this directory"));
}

#[test]
fn empty_check_and_verify_render_every_output_mode() {
    let fixture = Fixture::new("empty-gates");
    fixture.write("hardgate.toml", CUSTOM_CONFIG);

    for command in ["check", "verify"] {
        let json_output = run(fixture.as_ref(), &[command, "--format", "json"]);
        assert_success(&json_output, &format!("{command} json"));
        let report = json(&json_output);
        assert_eq!(report["passed"], true);
        assert!(report["advisories"].as_array().is_some());

        let agent = run(fixture.as_ref(), &[command, "--format", "agent"]);
        assert_success(&agent, &format!("{command} agent"));
        assert!(output_text(&agent).contains("Hardgate Passed"));

        let compact = run(fixture.as_ref(), &[command, "--format", "compact"]);
        assert_success(&compact, &format!("{command} compact"));
        assert!(output_text(&compact).contains("result: pass"));

        let summary = run(fixture.as_ref(), &[command, "--format", "summary"]);
        assert_success(&summary, &format!("{command} summary"));
        assert!(output_text(&summary).contains("Summary: 0 errors"));
    }
}

#[test]
fn scan_missing_file_returns_structured_json_error() {
    let fixture = Fixture::new("scan-missing");
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
