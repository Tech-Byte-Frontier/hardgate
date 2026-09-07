#[path = "common/cli.rs"]
mod cli;
use cli::{Fixture, assert_status, json, run, stderr, stdout};
use std::os::unix::fs::PermissionsExt;

const POLICY: &str =
    "[gate]\npreset = 'custom'\n[coverage]\nenabled = false\n[mutation]\nenabled = false\n";

fn executable(fixture: &Fixture, path: &str) {
    fixture.write(path, "#!/bin/sh\ntouch unexpected-execution\n");
    std::fs::set_permissions(fixture.join(path), std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn doctor_human_output_places_missing_setup_before_ready_tools() {
    let fixture = Fixture::new("cli064-edges", "doctor-human", Some(POLICY));
    fixture.write("hardgate.toml", &format!("{POLICY}\n[orchestration]\nformat_check = 'sh -c true'\nlint = 'nonexistent-lint-064'\nadditional_tests = ['sh -c true']\n"));
    let output = run(&fixture, &["doctor"]);
    assert_eq!(output.status.code(), Some(2));
    let text = stdout(&output);
    assert!(text.contains("setup or evidence incomplete"));
    assert!(text.find("missing lint").unwrap() < text.find("ready format_check").unwrap());
    fixture.write(
        "hardgate.toml",
        &format!("{POLICY}\n[orchestration]\nformat_check = 'sh -c true'\nlint = 'sh -c true'\n"),
    );
    let output = run(&fixture, &["doctor"]);
    assert_status(&output, true, "ready doctor");
    assert!(stdout(&output).contains("preflight ready"));
}

#[test]
fn doctor_reports_missing_mutation_paths_with_and_without_configuration() {
    let fixture = Fixture::new("cli064-edges", "doctor-mutation", None);
    for reports in [
        "",
        "reports = []",
        "reports = ['absent.json', 'other.json']",
    ] {
        fixture.write(
            "hardgate.toml",
            &format!("[gate]\npreset = 'custom'\n[mutation]\nenabled = true\n{reports}\n"),
        );
        let output = run(&fixture, &["doctor", "--json"]);
        assert_eq!(output.status.code(), Some(2));
        let report = json(&output);
        assert!(
            report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| check["name"] == "mutation evidence" && check["ready"] == false)
        );
        assert!(stdout(&output).contains(if reports.contains("absent") {
            "missing source-bound receipt"
        } else {
            "report path is not configured"
        }));
    }
}

#[test]
fn doctor_does_not_accept_directories_or_nonexecutable_launchers() {
    let fixture = Fixture::new("cli064-edges", "doctor-paths", None);
    fixture.write("tool", "not executable");
    for command in ["./tool", "./.git", ""] {
        fixture.write(
            "hardgate.toml",
            &format!(
                "{POLICY}\n[orchestration]\nformat_check = '{command}'\nlint = 'sh -c true'\n"
            ),
        );
        let output = run(&fixture, &["doctor", "--json"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(stdout(&output).contains("launcher unavailable"));
    }
    executable(&fixture, "tool");
    fixture.write(
        "hardgate.toml",
        &format!("{POLICY}\n[orchestration]\nformat_check = './tool'\nlint = 'sh -c true'\n"),
    );
    assert_status(&run(&fixture, &["doctor"]), true, "direct executable");
    assert!(!fixture.join("unexpected-execution").exists());
}

#[test]
fn doctor_checks_named_scripts_without_executing_package_manager() {
    let fixture = Fixture::new("cli064-edges", "doctor-scripts", None);
    executable(&fixture, "node_modules/.bin/pnpm");
    fixture.write("hardgate.toml", &format!("{POLICY}\n[orchestration]\nformat_check = 'pnpm run format-check'\nlint = 'sh -c true'\n"));
    for script in [
        serde_json::Value::Null,
        serde_json::json!(" "),
        serde_json::json!("touch unexpected-execution"),
    ] {
        fixture.write(
            "package.json",
            &serde_json::json!({"scripts":{"format-check":script}}).to_string(),
        );
        let output = run(&fixture, &["doctor", "--json"]);
        assert_eq!(
            output.status.success(),
            script.as_str().is_some_and(|text| !text.trim().is_empty()),
            "{}",
            stdout(&output)
        );
        assert!(!fixture.join("unexpected-execution").exists());
    }
    // Option-driven or unnamed manager commands remain explicitly unverified.
    for command in ["pnpm exec", "pnpm run", "pnpm --version"] {
        fixture.write(
            "hardgate.toml",
            &format!(
                "{POLICY}\n[orchestration]\nformat_check = '{command}'\nlint = 'sh -c true'\n"
            ),
        );
        let output = run(&fixture, &["doctor"]);
        assert_status(&output, true, "launcher-only preflight");
        assert!(stdout(&output).contains("arguments/scripts unverified"));
    }
}

#[test]
fn scoped_format_rejects_directories_and_symlink_escapes() {
    let fixture = Fixture::new("cli064-edges", "fmt-paths", None);
    let external = Fixture::new("cli064-edges", "external", None);
    external.write("external.ts", "original");
    std::os::unix::fs::symlink(external.join("external.ts"), fixture.join("escape.ts")).unwrap();
    fixture.write(
        "hardgate.toml",
        &format!("{POLICY}\n[orchestration]\nformat_files = 'sh formatter.sh {{files}}'\n"),
    );
    fixture.write("formatter.sh", "touch unexpected-execution\n");
    for path in [".", "escape.ts"] {
        let output = run(&fixture, &["fmt", path]);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("file inside"));
    }
    assert!(!fixture.join("unexpected-execution").exists());
    assert_eq!(
        std::fs::read_to_string(external.join("external.ts")).unwrap(),
        "original"
    );
}

#[test]
fn scoped_format_reports_missing_check_template_and_invalid_templates() {
    let fixture = Fixture::new("cli064-edges", "fmt-templates", Some(POLICY));
    fixture.write("file.ts", "original");
    let output = run(&fixture, &["fmt", "--check", "file.ts"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("format_check_files"));
    for template in ["sh formatter.sh", "{files}", "sh {files} {files}"] {
        fixture.write(
            "hardgate.toml",
            &format!("{POLICY}\n[orchestration]\nformat_files = '{template}'\n"),
        );
        let output = run(&fixture, &["fmt", "file.ts"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("exactly one standalone"));
    }
}

#[test]
fn changed_format_empty_index_is_noop_and_staged_deletion_is_skipped() {
    let fixture = Fixture::new("cli064-edges", "fmt-empty", Some(POLICY));
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&fixture.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "initial",
    ]);
    let output = run(&fixture, &["fmt", "--changed"]);
    assert_status(&output, true, "empty formatting selection");
    assert!(stdout(&output).contains("No changed files"));
    fixture.write("new.ts", "staged then deleted");
    git(&["add", "new.ts"]);
    std::fs::remove_file(fixture.join("new.ts")).unwrap();
    assert_status(
        &run(&fixture, &["fmt", "--changed"]),
        true,
        "deleted staged file",
    );
}

#[test]
fn unexecuted_mutants_do_not_claim_score_success_without_viable_kills() {
    let fixture = Fixture::new("cli064-edges", "mutation-failed-score", None);
    let keeper =
        hardgate::engines::MutationGatekeeper::new(&hardgate::config::MutationConfig::default());
    for outcomes in [vec!["NoCoverage"], vec!["Survived", "NoCoverage"]] {
        let mutants: Vec<_> = outcomes
            .into_iter()
            .map(|status| serde_json::json!({"status":status}))
            .collect();
        fixture.write(
            "mutation.json",
            &serde_json::json!({"files":{"file.ts":{"mutants":mutants}}}).to_string(),
        );
        let findings = keeper
            .evaluate_report(&fixture.join("mutation.json"))
            .unwrap();
        assert!(findings[0].message.starts_with("Score failed"));
        assert!(findings[0].recommendation.contains("file.ts:?:?"));
        assert!(
            findings
                .iter()
                .any(|finding| finding.metric == "Mutation Kill Rate")
        );
    }
}
