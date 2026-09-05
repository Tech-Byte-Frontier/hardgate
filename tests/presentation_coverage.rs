#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, json, run, stderr, stdout};
use std::process::{Command, Output};

const CONFIG: &str = "[gate]\npreset = 'custom'\n[budgets.functions]\nmax_parameters = 1\n";
const SOURCE: &str = "pub fn decide(first: i32, second: i32) -> i32 { first + second }\n";

fn fixture(tag: &str) -> Fixture {
    let fixture = Fixture::new("presentation-coverage", tag, Some(CONFIG));
    fixture.write("src/value.rs", SOURCE);
    fixture
}

fn run_with_env(fixture: &Fixture, args: &[&str], values: &[(&str, Option<&str>)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hardgate"));
    command.current_dir(&fixture.0).args(args);
    for (name, value) in values {
        match value {
            Some(value) => {
                command.env(name, value);
            }
            None => {
                command.env_remove(name);
            }
        }
    }
    command.output().expect("hardgate binary should run")
}

fn has_ansi(output: &Output) -> bool {
    stdout(output).contains('\u{1b}')
}

#[test]
fn color_controls_and_empty_environment_values_have_observable_effects() {
    let fixture = fixture("color");
    let always = run_with_env(
        &fixture,
        &["check", "--format", "terminal", "--color", "always"],
        &[("NO_COLOR", Some("1")), ("CLICOLOR_FORCE", Some("1"))],
    );
    assert_eq!(always.status.code(), Some(1));
    assert!(has_ansi(&always));

    let never = run_with_env(
        &fixture,
        &["check", "--format", "terminal", "--color", "never"],
        &[("NO_COLOR", None), ("CLICOLOR_FORCE", Some("1"))],
    );
    assert_eq!(never.status.code(), Some(1));
    assert!(!has_ansi(&never));

    let no_color = run_with_env(
        &fixture,
        &["check", "--format", "terminal", "--color", "auto"],
        &[("NO_COLOR", Some("1")), ("CLICOLOR_FORCE", None)],
    );
    assert_eq!(no_color.status.code(), Some(1));
    assert!(!has_ansi(&no_color));

    let forced = run_with_env(
        &fixture,
        &["check", "--format", "terminal", "--color", "auto"],
        &[("NO_COLOR", None), ("CLICOLOR_FORCE", Some("1"))],
    );
    assert_eq!(forced.status.code(), Some(1));
    assert!(has_ansi(&forced));

    let empty_values = run_with_env(
        &fixture,
        &["check", "--format", "terminal", "--color", "auto"],
        &[
            ("NO_COLOR", Some("")),
            ("CLICOLOR_FORCE", Some("")),
            ("CLICOLOR", Some("")),
            ("TERM", Some("")),
        ],
    );
    assert_eq!(empty_values.status.code(), Some(1));
    assert!(!has_ansi(&empty_values));
}

#[test]
fn timing_and_error_formats_preserve_observable_runtime_contracts() {
    let fixture = fixture("runtime");
    let timed = run(&fixture, &["--timing", "completions", "bash"]);
    cli::assert_status(&timed, true, "timed completions");
    assert!(stdout(&timed).contains("hardgate"));
    assert!(stderr(&timed).contains("hardgate: completions took "));

    let missing = run(&fixture, &["scan", "missing.rs", "--format", "json"]);
    assert_eq!(missing.status.code(), Some(2));
    let missing_report = json(&missing);
    assert_eq!(missing_report["schema_version"], 1);
    assert_eq!(missing_report["status"], "error");
    assert_eq!(missing_report["exit_code"], 2);
    assert_eq!(missing_report["stage"], "scan");
    assert!(stderr(&missing).is_empty());

    let json_args = run(&fixture, &["check", "--format=json", "--unknown-option"]);
    assert_eq!(json_args.status.code(), Some(2));
    assert_eq!(json(&json_args)["stage"], "arguments");
    assert!(stderr(&json_args).is_empty());

    let text_args = run(&fixture, &["check", "--unknown-option"]);
    assert_eq!(text_args.status.code(), Some(2));
    assert!(stdout(&text_args).is_empty());
    assert!(stderr(&text_args).contains("error:"));

    let help = run(&fixture, &["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(stdout(&help).contains("Usage:"));
}
