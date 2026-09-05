#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, json, run, stderr, stdout};
use std::process::Command;

const CONFIG: &str = "[gate]\npreset = 'custom'\n[budgets.functions]\nmax_parameters = 1\n";
const SOURCE: &str = "pub fn decide(first: i32, second: i32) -> i32 { first + second }\n";

fn fixture(tag: &str) -> Fixture {
    let fixture = Fixture::new("cli-process", tag, Some(CONFIG));
    fixture.write("src/value.rs", SOURCE);
    fixture
}

#[test]
fn exits_distinguish_policy_failure_missing_evidence_and_invalid_configuration() {
    let fixture = fixture("exit-codes");
    for format in ["terminal", "agent", "json", "compact", "summary"] {
        let args = ["check", "--format", format];
        assert_eq!(run(&fixture, &args).status.code(), Some(1), "{format}");
        fixture.write("src/value.rs", "pub fn answer() -> i32 { 17 }\n");
        assert_eq!(run(&fixture, &args).status.code(), Some(0), "{format}");
        fixture.write(
            "hardgate.toml",
            &format!("{CONFIG}\n[coverage]\nenabled = true\n"),
        );
        assert_eq!(run(&fixture, &args).status.code(), Some(2), "{format}");
        fixture.write("hardgate.toml", "malformed policy");
        let invalid = run(&fixture, &args);
        assert_eq!(invalid.status.code(), Some(2), "{format}");
        if format == "json" {
            assert_eq!(json(&invalid)["schema_version"], 1);
            assert!(
                invalid.stderr.is_empty(),
                "JSON errors must have one diagnostic"
            );
        }
        fixture.write("hardgate.toml", CONFIG);
        fixture.write("src/value.rs", SOURCE);
    }
}

#[test]
fn argument_errors_honor_requested_json_without_running_analysis() {
    let fixture = fixture("arguments");
    for args in [
        vec!["check", "--json", "--threads", "0"],
        vec!["scan", "--format=json"],
        vec!["check", "--format", "json", "--unknown-option"],
    ] {
        let output = run(&fixture, &args);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(json(&output)["stage"], "arguments");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn incomplete_report_records_and_unconfigured_freshness_use_exit_two() {
    let fixture = fixture("missing-records");
    fixture.write(
        "coverage.info",
        "SF:src/other.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    );
    for policy in [
        "[coverage]\nenabled = true\nreport = 'coverage.info'\n",
        "[generated]\nenabled = true\n",
    ] {
        fixture.write("hardgate.toml", &format!("{CONFIG}\n{policy}"));
        let output = run(&fixture, &["check", "--json"]);
        assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
        assert_eq!(json(&output)["passed"], false);
    }
}

#[test]
fn scan_exposes_passing_function_metrics_in_human_and_machine_output() {
    let fixture = fixture("metrics");
    fixture.write("src/value.rs", "pub fn answer() -> i32 { 17 }\n");
    for format in ["terminal", "agent", "json", "compact", "summary"] {
        let output = run(&fixture, &["scan", "src/value.rs", "--format", format]);
        cli::assert_status(&output, true, "passing scan metrics");
        if format == "json" {
            let report = json(&output);
            assert_eq!(report["functions"][0]["name"], "answer");
            assert_eq!(report["functions"][0]["cyclomatic"], 1);
        } else {
            assert!(stdout(&output).contains("answer: cyclomatic=1"));
        }
    }
}

#[test]
fn worker_limits_preserve_findings_and_timing_is_opt_in_stderr() {
    let fixture = fixture("threads");
    for index in 0..12 {
        fixture.write(
            &format!("src/value_{index}.rs"),
            &SOURCE.replace("decide", &format!("decide_{index}")),
        );
    }
    let mut single = json(&run(&fixture, &["check", "--json", "--threads", "1"]));
    let output = run(&fixture, &["check", "--json", "--threads", "4", "--timing"]);
    let mut parallel = json(&output);
    single.as_object_mut().unwrap().remove("duration_ms");
    parallel.as_object_mut().unwrap().remove("duration_ms");
    assert_eq!(single, parallel);
    assert!(stderr(&output).contains("check took"));
}

#[test]
fn color_flags_override_environment_and_json_stays_plain() {
    let fixture = fixture("color");
    for (color, format, ansi) in [
        ("always", "terminal", true),
        ("never", "terminal", false),
        ("always", "json", false),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_hardgate"))
            .current_dir(&fixture.0)
            .args(["check", "--format", format, "--color", color])
            .env("NO_COLOR", "1")
            .env("CLICOLOR_FORCE", "1")
            .output()
            .unwrap();
        assert_eq!(stdout(&output).contains('\u{1b}'), ansi);
    }
}

#[cfg(unix)]
#[test]
fn already_closed_stdout_never_panics_or_starts_mutation_children() {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::process::Stdio;
    let fixture = fixture("closed-stdout");
    fixture.write(
        "hardgate.toml",
        &format!("{CONFIG}\n[mutation]\nenabled = true\n"),
    );
    let cases = [
        vec!["check", "--format", "terminal"],
        vec!["check", "--format", "agent"],
        vec!["check", "--json"],
        vec!["check", "--compact"],
        vec!["check", "--summary"],
        vec!["scan", "src/value.rs", "--json"],
        vec!["config", "--format", "json"],
        vec!["check", "--json", "--threads", "0"],
        vec![
            "mutate",
            "--scoped",
            "src/value.rs",
            "--test-cmd",
            "touch unexpected-child",
        ],
    ];
    for args in cases {
        let (reader, writer) = UnixStream::pair().unwrap();
        drop(reader);
        let output = Command::new(env!("CARGO_BIN_EXE_hardgate"))
            .current_dir(&fixture.0)
            .args(&args)
            .stdout(Stdio::from(OwnedFd::from(writer)))
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
            .wait_with_output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(output.stderr.is_empty());
    }
    assert!(!fixture.join("unexpected-child").exists());
    assert_eq!(
        std::fs::read_to_string(fixture.join("src/value.rs")).unwrap(),
        SOURCE
    );
}
