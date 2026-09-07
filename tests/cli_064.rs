#[path = "common/cli.rs"]
mod cli;
use cli::{Fixture, json, run, stderr, stdout};

const POLICY: &str =
    "[gate]\npreset = 'custom'\n[coverage]\nenabled = false\n[mutation]\nenabled = false\n";

#[test]
fn doctor_is_read_only_and_distinguishes_launchers_from_evidence() {
    let fixture = Fixture::new("cli064", "doctor", None);
    fixture.write("hardgate.toml", "[gate]\npreset = 'custom'\n[coverage]\nenabled = true\nreport = 'absent.lcov'\n[orchestration]\ntest_cmd = \"sh -c 'touch must-not-exist'\"\nlint = 'hardgate-nonexistent-tool-064'\n");
    let output = run(&fixture, &["doctor", "--json"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let report = json(&output);
    assert_eq!(report["ready"], false);
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "tests" && item["ready"] == true)
    );
    assert!(stdout(&output).contains("missing source-bound receipt"));
    assert!(!fixture.join("must-not-exist").exists());
    assert!(!fixture.join(".hardgate").exists());
    if cfg!(target_os = "macos") {
        assert!(report["host"].as_str().unwrap().contains("Linux host"));
    }
}

#[test]
fn scoped_formatter_preserves_arguments_and_leaves_other_files_alone() {
    let fixture = Fixture::new("cli064", "fmt-files", None);
    fixture.write("hardgate.toml", &format!("{POLICY}\n[orchestration]\nformat = \"sh -c 'touch whole-project-ran'\"\nformat_files = 'sh formatter.sh {{files}}'\nformat_check_files = 'sh checker.sh {{files}}'\n"));
    fixture.write(
        "formatter.sh",
        "for file do printf 'formatted' > \"$file\"; done\n",
    );
    fixture.write(
        "checker.sh",
        "for file do test \"$(cat \"$file\")\" = formatted || exit 1; done\n",
    );
    let name = "a '$(touch injected)' file.ts";
    fixture.write(name, "original");
    fixture.write("untouched.ts", "unchanged");
    let output = run(&fixture, &["fmt", name]);
    cli::assert_status(&output, true, "scoped formatting");
    assert_eq!(
        std::fs::read_to_string(fixture.join(name)).unwrap(),
        "formatted"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.join("untouched.ts")).unwrap(),
        "unchanged"
    );
    assert!(!fixture.join("injected").exists());
    assert!(!fixture.join("whole-project-ran").exists());
    assert!(run(&fixture, &["fmt", "--check", name]).status.success());
}

#[test]
fn scoped_format_requires_explicit_file_aware_configuration() {
    let fixture = Fixture::new("cli064", "fmt-refusal", None);
    fixture.write(
        "hardgate.toml",
        &format!("{POLICY}\n[orchestration]\nformat = \"sh -c 'touch whole-project-ran'\"\n"),
    );
    fixture.write("file.ts", "original");
    let output = run(&fixture, &["fmt", "file.ts"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("format_files"));
    assert!(!fixture.join("whole-project-ran").exists());
}

#[test]
fn checks_offer_screenshot_guidance_and_allow_disposable_output() {
    let fixture = Fixture::new("cli064", "screenshot", None);
    fixture.write(
        "hardgate.toml",
        &format!(
            "{POLICY}\n[orchestration]\ntest_cmd = \"sh -c 'printf screenshot > screenshot.png'\"\n"
        ),
    );
    let output = run(
        &fixture,
        &["check", "--checks", "tests", "--format", "agent"],
    );
    assert!(!output.status.success());
    assert!(
        stdout(&output).contains("testInfo.outputPath"),
        "{}",
        stdout(&output)
    );
    assert!(!fixture.join("screenshot.png").exists());
    fixture.write("hardgate.toml", &format!("{POLICY}\n[orchestration]\ntest_cmd = \"sh -c 'printf screenshot > \\\"$TMPDIR/screenshot.png\\\"'\"\n"));
    let output = run(
        &fixture,
        &[
            "check",
            "--checks",
            "tests",
            "--format",
            "json",
            "--progress",
            "jsonl",
        ],
    );
    assert!(
        output.status.success(),
        "{} {}",
        stdout(&output),
        stderr(&output)
    );
    assert_eq!(json(&output)["passed"], true);
    let events: Vec<serde_json::Value> = stderr(&output)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    assert!(
        events
            .iter()
            .any(|event| event["stage"] == "test" && event["timeout_ms"].is_number())
    );
}

#[test]
fn parser_rejection_has_coordinates_and_does_not_claim_compiler_invalidity() {
    let fixture = Fixture::new("cli064", "parser", Some(POLICY));
    fixture.write(
        "sample.ts",
        "const before = 1;\nconst result = original<typeof import('react-dom/client')>();\n",
    );
    let output = run(&fixture, &["scan", "sample.ts", "--format", "json"]);
    let report = json(&output);
    let text = stdout(&output);
    assert!(text.contains("Hardgate parser could not analyze"), "{text}");
    assert!(text.contains("source validity unconfirmed"));
    assert!(text.contains("unsupported parser syntax"));
    let diagnostics = report["diagnostics"].as_array().unwrap();
    let failure = diagnostics
        .iter()
        .find(|item| item["rule_id"] == "HG-ORCHESTRATION-PARSE-SOURCE")
        .unwrap();
    assert_eq!(failure["locations"][0]["line"], 2);
    assert!(failure["locations"][0]["column"].as_u64().unwrap() > 0);
    fixture.write(
        "sample.ts",
        "type Client = typeof import('react-dom/client');\nconst result = original<Client>();\n",
    );
    let output = run(&fixture, &["scan", "sample.ts", "--format", "json"]);
    assert!(!stdout(&output).contains("Hardgate parser could not analyze"));
}

#[test]
fn changed_format_handles_staged_unstaged_untracked_and_deleted_files() {
    let fixture = Fixture::new("cli064", "changed", Some(POLICY));
    fixture.write(
        "hardgate.toml",
        &format!("{POLICY}\n[orchestration]\nformat_files = 'sh formatter.sh {{files}}'\n"),
    );
    fixture.write(
        "formatter.sh",
        "for file do printf 'formatted' > \"$file\"; done\n",
    );
    for file in ["staged.ts", "unstaged.ts", "deleted.ts", "untouched.ts"] {
        fixture.write(file, "original");
    }
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
    fixture.write("staged.ts", "staged");
    git(&["add", "staged.ts"]);
    fixture.write("unstaged.ts", "unstaged");
    fixture.write("new.ts", "untracked");
    std::fs::remove_file(fixture.join("deleted.ts")).unwrap();
    let output = run(&fixture, &["fmt", "--changed"]);
    assert!(output.status.success(), "{}", stderr(&output));
    for file in ["staged.ts", "unstaged.ts", "new.ts"] {
        assert_eq!(
            std::fs::read_to_string(fixture.join(file)).unwrap(),
            "formatted"
        );
    }
    assert_eq!(
        std::fs::read_to_string(fixture.join("untouched.ts")).unwrap(),
        "original"
    );
    assert!(!fixture.join("deleted.ts").exists());
}

#[test]
fn agent_groups_inventory_advisories_after_blocking_findings() {
    let mut report = hardgate::diagnostics::GateReport::default();
    for file in ["a.css", "b.css", "c.css", "d.css"] {
        report.advisories.push(format!("role Source: `{file}` is a recognized inventory source without AST parser; validated for file budgets, suppressions, and invariants."));
    }
    report
        .orchestration_violations
        .push(hardgate::engines::OrchestrationViolation {
            step: "test".into(),
            command: "test".into(),
            exit_code: Some(1),
            output: "blocking failure".into(),
            recommendation: "next command".into(),
        });
    let agent = report.render_agent();
    assert_eq!(
        agent
            .matches("Recognized inventory source without AST parser")
            .count(),
        1
    );
    assert!(agent.contains("4 occurrences"));
    assert!(agent.find("next command").unwrap() < agent.find("Advisory:").unwrap());
    assert_eq!(serde_json::from_str::<serde_json::Value>(&report.render_json().unwrap()).unwrap()["advisories"].as_array().unwrap().len(), 4);
}

#[test]
fn passing_mutation_score_still_identifies_unexecuted_locations() {
    let fixture = Fixture::new("cli064", "mutation", None);
    fixture.write("mutation.json", r#"{"files":{"src/main.ts":{"mutants":[{"id":"0","status":"Killed"},{"id":"1","status":"NoCoverage","location":{"start":{"line":12,"column":4}}},{"id":"2","status":"NoCoverage","location":{"start":{"line":20,"column":2}}}]}}}"#);
    let keeper =
        hardgate::engines::MutationGatekeeper::new(&hardgate::config::MutationConfig::default());
    let violations = keeper
        .evaluate_report(&fixture.join("mutation.json"))
        .unwrap();
    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.starts_with("Score passed"));
    assert!(
        violations[0]
            .message
            .contains("evidence incomplete: 2 unexecuted mutants")
    );
    let mut report = hardgate::diagnostics::GateReport::default();
    report.mutation_violations = violations;
    for rendered in [
        report.render_agent(),
        report.render_terminal(),
        report.render_json().unwrap(),
    ] {
        assert!(rendered.contains("Score passed"));
        assert!(rendered.contains("src/main.ts:12:4"));
        assert!(rendered.contains("src/main.ts:20:2"));
    }
}

#[test]
fn long_run_emits_live_progress_before_command_completion() {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    let fixture = Fixture::new("cli064", "live-progress", None);
    fixture.write("hardgate.toml", &format!("{POLICY}\n[orchestration]\nformat = \"sh -c 'printf \\\"Mutation: 2/4 tested\\\\n\\\"; sleep 12'\"\ntimeout_secs = 15\n"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .args(["fmt"])
        .current_dir(&fixture.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut status = String::new();
    for line in std::io::BufReader::new(child.stderr.take().unwrap()).lines() {
        let line = line.unwrap();
        if line.contains("phase=format") {
            status = line;
            break;
        }
    }
    assert!(status.contains("elapsed=10s timeout=15s"), "{status}");
    assert!(status.contains("Mutation: 2/4 tested"), "{status}");
    assert!(
        child.try_wait().unwrap().is_none(),
        "status must arrive while command is running"
    );
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(stdout(&output).contains("Mutation: 2/4 tested"));
}

#[test]
fn doctor_checks_pnpm_exec_tool_without_running_or_downloading_it() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new("cli064", "doctor-pnpm", None);
    fixture.write("hardgate.toml", &format!("{POLICY}\n[orchestration]\nformat_check = 'pnpm exec missing-formatter-064'\nlint = 'sh -c true'\n"));
    fixture.write("node_modules/.bin/pnpm", "#!/bin/sh\ntouch must-not-run\n");
    std::fs::set_permissions(
        fixture.join("node_modules/.bin/pnpm"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let output = run(&fixture, &["doctor", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).contains("delegated tool"));
    assert!(!fixture.join("must-not-run").exists());
    fixture.write(
        "node_modules/.bin/missing-formatter-064",
        "#!/bin/sh\nexit 0\n",
    );
    std::fs::set_permissions(
        fixture.join("node_modules/.bin/missing-formatter-064"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let output = run(&fixture, &["doctor", "--json"]);
    cli::assert_status(&output, true, "doctor local launchers");
    assert_eq!(json(&output)["ready"], true);
    assert!(!fixture.join("must-not-run").exists());
}
