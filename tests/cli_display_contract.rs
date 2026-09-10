#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, json, run, stdout};

const POLICY: &str = "[gate]\npreset='custom'\n[budgets.functions]\nmax_parameters=1\n[clones]\nmin_lines=2\nmin_tokens=8\n";
const SOURCE: &str = "export function combine(first: number, second: number) {\n  const total = first + second;\n  return total;\n}\n";

fn fixture(tag: &str) -> Fixture {
    let fixture = Fixture::new("display-contract", tag, Some(POLICY));
    fixture.write("src/first.ts", SOURCE);
    fixture.write("src/second.ts", SOURCE);
    fixture
}

#[test]
fn every_format_keeps_the_complete_verdict_when_no_diagnostics_are_displayed() {
    let fixture = fixture("limits");
    let full = json(&run(&fixture, &["check", "--checks", "policy", "--json"]));
    assert!(full["summary"]["total_errors"].as_u64().unwrap() >= 3);
    for format in ["terminal", "agent", "json", "compact", "summary"] {
        let output = run(
            &fixture,
            &[
                "check",
                "--checks",
                "policy",
                "--format",
                format,
                "--max-diagnostics",
                "0",
            ],
        );
        assert_eq!(
            output.status.code(),
            Some(1),
            "{format}: {}",
            stdout(&output)
        );
        if format == "json" {
            let limited = json(&output);
            assert_eq!(limited["summary"], full["summary"]);
            assert_eq!(limited["execution"], full["execution"]);
            assert_eq!(limited["passed"], false);
            assert_eq!(limited["shown"], 0);
            assert_eq!(limited["omitted"], full["total"]);
            assert!(limited["diagnostics"].as_array().unwrap().is_empty());
            for (key, value) in limited.as_object().unwrap() {
                if key.ends_with("_violations") {
                    assert!(value.as_array().unwrap().is_empty(), "{key}");
                }
            }
        } else {
            assert!(stdout(&output).contains("fail"), "{format}");
        }
    }
}

#[test]
fn display_limit_is_shared_across_categories_and_reports_stable_rule_ids() {
    let fixture = fixture("categories");
    let report = json(&run(
        &fixture,
        &[
            "check",
            "--checks",
            "policy",
            "--json",
            "--max-diagnostics",
            "2",
        ],
    ));
    assert_eq!(report["shown"], 2);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics[0]["rule_id"], "HG-COMPLEXITY-PARAMETERS");
    assert_eq!(diagnostics[1]["rule_id"], "HG-COMPLEXITY-PARAMETERS");
    assert!(report["clone_violations"].as_array().unwrap().is_empty());
    assert!(report["summary"]["clones"].as_u64().unwrap() > 0);
}

#[cfg(unix)]
#[test]
fn freshness_fixes_are_blocked_and_excerpts_retain_analyzed_bytes() {
    let fixture = fixture("snapshot");
    let original = std::fs::read_to_string(fixture.join("src/first.ts")).unwrap();
    fixture.write("hardgate.toml", &format!("{POLICY}\n[generated]\nenabled=true\nfreshness_command=\"sh -c 'printf changed > src/first.ts'\"\n"));
    let output = run(
        &fixture,
        &["check", "--checks", "policy", "--json", "--snippets"],
    );
    cli::assert_status(&output, false, "snapshot findings");
    let report = json(&output);
    let clone = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["rule_id"] == "HG-CLONE-DUPLICATE-BLOCK")
        .unwrap();
    let excerpts = clone["excerpts"].as_array().unwrap();
    assert_eq!(excerpts.len(), 2);
    assert!(excerpts.iter().all(|excerpt| {
        excerpt["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line.as_str().unwrap().contains("combine"))
    }));
    assert_eq!(
        std::fs::read_to_string(fixture.join("src/first.ts")).unwrap(),
        original
    );
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["step"] == "generated-freshness")
    );
    assert!(cli::stderr(&output).contains("workspace lifecycle=failed"));
}

#[test]
fn snippets_are_opt_in_and_conflicting_controls_fail_as_json() {
    let fixture = fixture("opt-in");
    let ordinary = json(&run(&fixture, &["scan", "src/first.ts", "--json"]));
    assert!(
        ordinary["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|diagnostic| diagnostic["excerpts"].as_array().unwrap().is_empty())
    );
    let snippet = json(&run(
        &fixture,
        &["scan", "src/first.ts", "--json", "--snippets"],
    ));
    assert!(snippet["snippet_bytes"].as_u64().unwrap() > 0);
    let conflict = run(
        &fixture,
        &[
            "check",
            "--checks",
            "policy",
            "--json",
            "--snippets",
            "--no-snippets",
        ],
    );
    assert_eq!(conflict.status.code(), Some(2));
    assert_eq!(json(&conflict)["stage"], "arguments");
}
