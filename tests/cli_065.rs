#[path = "common/cli.rs"]
mod cli;
use cli::{Fixture, assert_status, json, run, stdout};

const POLICY: &str = "[gate]\npreset = 'custom'\n[coverage]\nenabled = false\n[mutation]\nenabled = false\n[clones]\nenabled = false\n";

#[test]
fn ignored_inventory_migration_does_not_poison_completed_source_analysis() {
    for severity in ["ignore", "warning", "error"] {
        let fixture = Fixture::new(
            "cli065",
            severity,
            Some(&format!(
                "{POLICY}\n[roles.migration]\nseverity = '{severity}'\n[orchestration]\nformat_check = 'sh -c true'\nlint = 'sh -c true'\n"
            )),
        );
        fixture.write("src/main.ts", "export const value = 1;\n");
        fixture.write(
            "supabase/migrations/001_init.sql",
            "create table example (id integer);\n",
        );
        let output = run(&fixture, &["check", "--json"]);
        let report = json(&output);
        let engine = report["execution"]["engines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == "complexity")
            .unwrap();
        if severity == "ignore" {
            assert_eq!(report["accepted"], true, "{}", stdout(&output));
            assert_eq!(engine["state"], "completed");
            assert!(stdout(&output).contains("Inventoried without AST"));
        } else {
            assert_eq!(report["accepted"], false);
            assert_eq!(engine["state"], "incomplete");
            assert!(
                engine["reason"]
                    .as_str()
                    .unwrap()
                    .contains("supabase/migrations/001_init.sql")
            );
            for flags in [
                vec!["--compact"],
                vec!["--format", "agent", "--details"],
                vec!["--summary"],
                vec![],
            ] {
                let mut args = vec!["check"];
                args.extend(flags);
                let human = stdout(&run(&fixture, &args));
                assert!(human.contains("Hardgate Incomplete"), "{human}");
                assert!(!human.contains("✅ **Hardgate Passed**"));
            }
        }
    }
}

#[test]
fn policy_output_flag_matrix_saves_exact_stdout_and_rejects_failed_writes() {
    let fixture = Fixture::new("cli065", "output", Some(POLICY));
    fixture.write("src/main.ts", "export const value = 1;\n");
    for flags in [
        vec!["--format", "json"],
        vec!["--format", "json", "--summary"],
        vec!["--compact"],
        vec!["--summary"],
    ] {
        let mut args = vec!["check", "--checks", "policy", "--output", "report.out"];
        args.extend(flags);
        let output = run(&fixture, &args);
        assert_status(&output, true, "policy output matrix");
        assert_eq!(
            std::fs::read(fixture.join("report.out")).unwrap(),
            output.stdout
        );
        std::fs::remove_file(fixture.join("report.out")).unwrap();
    }
    let output = run(
        &fixture,
        &["check", "--checks", "policy", "--json", "--output", "src"],
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn missing_formatter_is_explicitly_unevaluated() {
    let fixture = Fixture::new("cli065", "formatter", Some(POLICY));
    fixture.write("src/main.ts", "export const value = 1;\n");
    let output = run(&fixture, &["check", "--checks", "format", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    let report = json(&output);
    assert_eq!(report["accepted"], false);
    assert!(
        stdout(&output)
            .contains("No formatter configured or detected; formatting was not evaluated.")
    );
}

#[test]
fn pnpm_verification_environment_cannot_install_and_propagates_tool_failure() {
    let fixture = Fixture::new("cli065", "pnpm-env", Some(POLICY));
    fixture.write("verify.sh", "#!/bin/sh\n[ \"$pnpm_config_verify_deps_before_run\" = false ] || exit 31\n[ \"$npm_config_verify_deps_before_run\" = false ] || exit 32\nexit 7\n");
    fixture.write(
        "hardgate.toml",
        &format!(
            "{POLICY}\n[orchestration]\nrequire_isolation = true\nformat_check = 'sh verify.sh'\n"
        ),
    );
    let output = run(&fixture, &["check", "--checks", "format", "--json"]);
    let report = json(&output);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert_eq!(report["orchestration_violations"][0]["exit_code"], 7);
}
