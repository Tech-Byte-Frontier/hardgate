#[path = "common/cli.rs"]
mod cli;
#[path = "common/fs_git.rs"]
mod fs_git;

use cli::{Fixture, run, stderr, stdout};
use fs_git::{commit_baseline, init_repo, write};
use serde_json::Value;
use std::process::Output;

const MUTATION_CONFIG: &str = r#"
[gate]
preset = "custom"
strict = true
enforce_classified_sources = true

[mutation]
enabled = true
min_score = 0.0
timeout_secs = 2
max_mutants = 1
"#;

const DISABLED_CONFIG: &str = r#"
[gate]
preset = "custom"
strict = true

[mutation]
enabled = false
"#;

const SOURCE: &str = "pub fn accepts(value: bool) -> bool { value == true }\n";

fn git_baseline(fixture: &Fixture) {
    init_repo(fixture.as_ref());
    commit_baseline(fixture.as_ref(), "baseline");
}

fn assert_json(output: &Output) -> Value {
    assert!(
        !output.stdout.is_empty(),
        "expected JSON stdout; stderr: {}",
        stderr(output)
    );
    cli::json(output)
}

fn setup_failure(output: &Output, message: &str) {
    assert!(!output.status.success());
    let report = assert_json(output);
    assert_eq!(report["stage"], "setup");
    assert_eq!(report["kind"], "setup-error");
    assert!(
        report["message"]
            .as_str()
            .is_some_and(|actual| actual.contains(message)),
        "setup error did not contain {message:?}: {report}"
    );
}

#[test]
fn disabled_mutation_has_agent_and_json_noop_contracts() {
    let fixture = Fixture::with_files(
        "mutate-command",
        "disabled-output",
        DISABLED_CONFIG,
        &[("src/lib.rs", SOURCE)],
    );

    let agent = run(fixture.as_ref(), &["mutate", "--format", "agent"]);
    assert!(agent.status.success(), "{}", stderr(&agent));
    assert!(stdout(&agent).contains("mutation testing is disabled"));

    let json = run(fixture.as_ref(), &["mutate", "--json"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let report = assert_json(&json);
    assert_eq!(report["status"], "noop");
    assert_eq!(report["stage"], "policy");
    assert_eq!(report["kind"], "disabled");
}

#[test]
fn empty_discovery_fails_full_runs_but_diff_is_a_machine_noop() {
    let fixture = Fixture::with_files("mutate-command", "empty-discovery", MUTATION_CONFIG, &[]);
    git_baseline(&fixture);

    let full = run(fixture.as_ref(), &["mutate", "--format", "agent"]);
    assert!(!full.status.success());
    assert!(stderr(&full).contains("full/native runs require at least one production target"));

    let diff = run(fixture.as_ref(), &["mutate", "--diff", "--format", "json"]);
    assert!(diff.status.success(), "{}", stderr(&diff));
    let report = assert_json(&diff);
    assert_eq!(report["status"], "noop");
    assert_eq!(report["stage"], "selection");
    assert_eq!(report["kind"], "no-changed-targets");
}

#[test]
fn explicit_directory_scope_honors_maximum_and_agent_survivor_warning() {
    let fixture = Fixture::with_files(
        "mutate-command",
        "scoped-selection",
        MUTATION_CONFIG,
        &[("src/first.rs", SOURCE), ("src/second.rs", SOURCE)],
    );
    let output = run(
        fixture.as_ref(),
        &[
            "mutate",
            "--scoped",
            "src",
            "--test-cmd",
            "true",
            "--max-mutants",
            "1",
            "--format",
            "agent",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("across 2 source files"), "{text}");
    assert!(text.contains("- Evaluated: 1"), "{text}");
    assert!(text.contains("Survived Mutant"), "{text}");
    assert!(text.contains("Verdict: PASSED"), "{text}");
}

#[test]
fn max_and_timeout_zero_are_typed_setup_failures() {
    let fixture = Fixture::with_files(
        "mutate-command",
        "zero-options",
        MUTATION_CONFIG,
        &[("src/lib.rs", SOURCE)],
    );

    let max = run(
        fixture.as_ref(),
        &[
            "mutate",
            "--scoped",
            "src/lib.rs",
            "--test-cmd",
            "true",
            "--max-mutants",
            "0",
            "--format",
            "json",
        ],
    );
    setup_failure(&max, "max_mutants must be greater than zero");

    let timeout = run(
        fixture.as_ref(),
        &[
            "mutate",
            "--scoped",
            "src/lib.rs",
            "--test-cmd",
            "true",
            "--timeout",
            "0",
            "--max-mutants",
            "1",
            "--format",
            "json",
        ],
    );
    setup_failure(&timeout, "timeout_secs must be greater than zero");
}

#[test]
fn automatic_javascript_full_suite_requires_explicit_safe_timeout() {
    let fixture = Fixture::with_files(
        "mutate-command",
        "javascript-timeout",
        MUTATION_CONFIG,
        &[("src/value.ts", "export const value = true;\n")],
    );
    write(
        fixture.as_ref(),
        "package.json",
        r#"{"packageManager":"npm@10","scripts":{"test":"node scripts/run-tests.mjs"}}"#,
    );
    let output = run(
        fixture.as_ref(),
        &["mutate", "--scoped", "src/value.ts", "--format", "json"],
    );
    setup_failure(&output, "automatic JavaScript full-suite selection");
}

#[test]
fn malformed_javascript_resolution_is_typed_in_agent_mode() {
    let fixture = Fixture::with_files(
        "mutate-command",
        "javascript-resolution",
        MUTATION_CONFIG,
        &[("src/value.ts", "export const value = true;\n")],
    );
    write(fixture.as_ref(), "package.json", "{\n");
    let output = run(
        fixture.as_ref(),
        &["mutate", "--scoped", "src/value.ts", "--format", "agent"],
    );
    assert!(!output.status.success());
    assert!(stderr(&output).contains("malformed JavaScript package manifest"));
}

#[test]
fn explicit_unsupported_scope_fails_before_mutant_generation() {
    let config = format!(
        "{MUTATION_CONFIG}\n[[classification.rules]]\nglob = \"src/value.custom\"\nrole = \"source\"\n"
    );
    let fixture = Fixture::with_files(
        "mutate-command",
        "unsupported-scope",
        &config,
        &[("src/value.custom", "value == true\n")],
    );
    let output = run(
        fixture.as_ref(),
        &["mutate", "--scoped", "src/value.custom", "--format", "json"],
    );
    setup_failure(&output, "no AST mutator");
}

#[test]
fn baseline_failure_is_silent_json_but_typed_in_terminal_output() {
    let fixture = Fixture::with_files(
        "mutate-command",
        "baseline-formats",
        MUTATION_CONFIG,
        &[("src/lib.rs", SOURCE)],
    );
    let json = run(
        fixture.as_ref(),
        &[
            "mutate",
            "--scoped",
            "src/lib.rs",
            "--test-cmd",
            "false",
            "--max-mutants",
            "1",
            "--format",
            "json",
        ],
    );
    assert!(!json.status.success());
    let report = assert_json(&json);
    assert_eq!(report["stage"], "baseline");
    assert_eq!(report["kind"], "test-failure");

    let terminal = run(
        fixture.as_ref(),
        &[
            "mutate",
            "--scoped",
            "src/lib.rs",
            "--test-cmd",
            "hardgate-command-that-does-not-exist",
            "--max-mutants",
            "1",
        ],
    );
    assert!(!terminal.status.success());
    let text = stderr(&terminal);
    assert!(text.contains("unmutated baseline RunnerError"), "{text}");
    assert!(text.contains("Failed to execute"), "{text}");
}

#[test]
fn diff_scope_distinguishes_unchanged_and_changed_sources() {
    let fixture = Fixture::with_files(
        "mutate-command",
        "diff-variants",
        MUTATION_CONFIG,
        &[("src/lib.rs", SOURCE)],
    );
    git_baseline(&fixture);

    let unchanged = run(
        fixture.as_ref(),
        &[
            "mutate",
            "--diff",
            "--scoped",
            "src/lib.rs",
            "--format",
            "json",
        ],
    );
    assert!(unchanged.status.success(), "{}", stderr(&unchanged));
    let noop = assert_json(&unchanged);
    assert_eq!(noop["kind"], "no-changed-targets");

    write(
        fixture.as_ref(),
        "src/lib.rs",
        "pub fn accepts(value: bool) -> bool { value != true }\n",
    );
    let changed = run(
        fixture.as_ref(),
        &[
            "mutate",
            "--diff",
            "--scoped",
            "src/lib.rs",
            "--test-cmd",
            "true",
            "--max-mutants",
            "1",
            "--format",
            "json",
        ],
    );
    assert!(changed.status.success(), "{}", stderr(&changed));
    let report = assert_json(&changed);
    assert_eq!(report["stats"]["total"], 1);
}
