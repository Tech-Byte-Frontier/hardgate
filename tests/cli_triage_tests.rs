#[path = "common/cli.rs"]
mod cli;
use cli::{Fixture, json, run, stdout};

const POLICY: &str = "[gate]\npreset='custom'\n[budgets.functions]\nmax_parameters=1\n[clones]\nmin_lines=2\nmin_tokens=8\n";
const SOURCE: &str = "export function combine(first: number, second: number) {\n  const total = first + second;\n  return total;\n}\n";

fn fixture(tag: &str) -> Fixture {
    let fixture = Fixture::new("triage", tag, Some(POLICY));
    fixture.write("src/a.ts", SOURCE);
    fixture.write("src/b.ts", SOURCE);
    fixture
}

#[test]
fn engine_filter_precedes_limit_and_preserves_full_verdict_and_json() {
    let f = fixture("filter");
    let full = json(&run(&f, &["check", "--checks", "policy", "--json"]));
    let output = run(
        &f,
        &[
            "check",
            "--checks",
            "policy",
            "--engine",
            "clones",
            "--max-diagnostics",
            "1",
            "--json",
        ],
    );
    cli::assert_status(&output, false, "triage preserves failure");
    assert_eq!(output.status.code(), Some(1));
    let view = json(&output);
    assert_eq!(view["execution"], full["execution"]);
    assert_eq!(view["summary"], full["summary"]);
    assert_eq!(view["partial"], true);
    assert_eq!(view["accepted"], false);
    assert_eq!(view["shown"], 1);
    assert_eq!(
        view["diagnostics"][0]["rule_id"],
        "HG-CLONE-DUPLICATE-BLOCK"
    );
    assert_eq!(
        view["diagnostics"][0]["locations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let no_match = run(
        &f,
        &[
            "check", "--checks", "policy", "--engine", "mutation", "--format", "agent",
        ],
    );
    assert_eq!(no_match.status.code(), Some(1));
    let text = stdout(&no_match);
    for required in [
        "Hardgate Failed",
        "partial",
        "Omitted requirements",
        "Displayed: 0/",
        "Inspect omitted findings",
    ] {
        assert!(text.contains(required), "{text}");
    }
}

#[test]
fn compact_display_saves_complete_json_and_replays_original_excerpts() {
    let f = fixture("save");
    let output = run(
        &f,
        &[
            "check",
            "--checks",
            "policy",
            "--engine",
            "clones",
            "--compact",
            "--snippets",
            "--max-diagnostics",
            "1",
            "--report-json",
            "full.json",
            "--output",
            "view.txt",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("src/a.ts:1-4 <-> src/b.ts:1-4"), "{text}");
    assert!(text.contains("tokens"));
    assert_eq!(std::fs::read_to_string(f.join("view.txt")).unwrap(), text);
    let full: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.join("full.json")).unwrap()).unwrap();
    assert!(full["complexity_violations"].as_array().unwrap().len() >= 2);
    assert_eq!(full["shown"], full["total"]);
    assert_eq!(full["schema_version"], 1);
    f.write("src/a.ts", "changed live source\n");
    std::fs::remove_file(f.join("src/b.ts")).unwrap();
    let inspection = run(
        &f,
        &[
            "report",
            "full.json",
            "--engine",
            "clones",
            "--format",
            "agent",
            "--snippets",
        ],
    );
    assert_eq!(inspection.status.code(), Some(1));
    let inspected = stdout(&inspection);
    assert!(
        inspected.contains("const total = first + second"),
        "{inspected}"
    );
    assert!(!inspected.contains("changed live source"));
}

#[test]
fn failures_remain_visible_with_no_matches_and_zero_limit() {
    let f = fixture("failures");
    f.write("hardgate.toml", &format!("{POLICY}\n[orchestration]\nformat_check='true'\nlint=\"sh -c 'echo lint-broke; exit 1'\"\n[coverage]\nenabled=true\nreport='missing.info'\n"));
    for format in ["agent", "compact", "json"] {
        let output = run(
            &f,
            &[
                "check",
                "--engine",
                "mutation",
                "--max-diagnostics",
                "0",
                "--format",
                format,
            ],
        );
        assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
        if format == "json" {
            let value = json(&output);
            assert_eq!(value["accepted"], false);
            assert_eq!(value["partial"], false);
            assert_eq!(value["shown"], 0);
            assert!(value["failures"].as_array().unwrap().len() >= 2);
        } else {
            let text = stdout(&output);
            for required in [
                "Hardgate Incomplete",
                "HG-ORCHESTRATION-COVERAGE-REPORT",
                "lint-broke",
                "Displayed: 0/",
            ] {
                assert!(text.contains(required), "{text}");
            }
        }
    }
}

#[test]
fn unsupported_analysis_names_every_file_without_inventing_lines() {
    let f = Fixture::new("triage", "unsupported", Some("[gate]\npreset='custom'\n"));
    for (path, text) in [
        ("one.go", "pass\n"),
        ("nested/two.go", "pass\n"),
        ("settings.yaml", "name: app\n"),
        ("styles.css", "body { color: red; }\n"),
    ] {
        f.write(path, text);
    }
    let output = run(&f, &["check", ".", "--checks", "policy", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    let failures: Vec<_> = value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["rule_id"] == "HG-ORCHESTRATION-UNSUPPORTED-SOURCE")
        .collect();
    assert_eq!(failures.len(), 2, "{value}");
    let paths: std::collections::BTreeSet<_> = failures
        .iter()
        .map(|v| {
            assert_eq!(v["locations"].as_array().unwrap().len(), 1);
            assert!(v["locations"][0]["line"].is_null());
            assert!(v["locations"][0]["end_line"].is_null());
            v["locations"][0]["file"].as_str().unwrap()
        })
        .collect();
    assert_eq!(paths, ["one.go", "nested/two.go"].into_iter().collect());
    let human = stdout(&run(
        &f,
        &[
            "check",
            ".",
            "--checks",
            "policy",
            "--engine",
            "clones",
            "--compact",
        ],
    ));
    assert!(
        human.contains("one.go") && human.contains("nested/two.go"),
        "{human}"
    );
}

#[test]
fn clone_execution_selector_error_explains_policy_and_display_choices() {
    let f = fixture("selector");
    let output = run(&f, &["check", "--checks", "clones", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    let text = value.to_string();
    for required in [
        "clones -> policy",
        "--checks policy",
        "--engine clones",
        "partial",
    ] {
        assert!(text.contains(required), "{text}");
    }
}

#[test]
fn output_aliases_fail_before_overwriting_either_report() {
    let f = fixture("aliases");
    f.write("saved.json", "preserve this file");
    let output = run(
        &f,
        &[
            "check",
            "--checks",
            "policy",
            "--report-json",
            "saved.json",
            "--output",
            "./saved.json",
            "--json",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).contains("different paths"));
    assert_eq!(
        std::fs::read_to_string(f.join("saved.json")).unwrap(),
        "preserve this file"
    );
}

#[test]
fn limited_saved_checks_remain_partial_views_and_never_load_live_excerpts() {
    let f = fixture("saved-limit");
    let output = run(
        &f,
        &[
            "check",
            "--checks",
            "policy",
            "--json",
            "--max-diagnostics",
            "1",
            "--output",
            "limited.json",
            "--report-json",
            "complete.json",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let full: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.join("complete.json")).unwrap()).unwrap();
    f.write(
        "src/a.ts",
        "live bytes must not be presented as captured evidence\n",
    );
    let inspection = run(
        &f,
        &[
            "report",
            "complete.json",
            "--engine",
            "clones",
            "--snippets",
            "--format",
            "agent",
        ],
    );
    assert!(stdout(&inspection).contains("Excerpts unavailable"));
    assert!(!stdout(&inspection).contains("live bytes"));
    let limited = json(&run(
        &f,
        &[
            "report",
            "limited.json",
            "--max-diagnostics",
            "0",
            "--json",
            "--output",
            "resaved.json",
        ],
    ));
    assert_eq!(limited["summary"], full["summary"]);
    assert_eq!(limited["inspection"]["displayed_errors"], 0);
    assert_eq!(limited["inspection"]["filtered"], true);
    let comparison = json(&run(
        &f,
        &[
            "report",
            "compare",
            "resaved.json",
            "resaved.json",
            "--json",
        ],
    ));
    assert_eq!(comparison["equivalent"], false);
    assert!(comparison.to_string().contains("Filtered report views"));
}

#[test]
fn no_matching_findings_can_pass_without_hiding_advisories_or_partial_scope() {
    let f = Fixture::new(
        "triage",
        "advisories",
        Some(
            "[gate]\npreset='balanced'\n[orchestration]\nformat_check='true'\nlint='true'\n[clones]\nmin_lines=2\nmin_tokens=8\n",
        ),
    );
    // Exceed balanced detection minima (8 lines / 80 tokens), while remaining
    // below the blocking minima (15 lines / 150 tokens).
    let source = "export function transform(input: number) {\n  let value = input;\n  value = value * 2 + 3;\n  value = value * 4 + 5;\n  value = value * 6 + 7;\n  value = value * 8 + 9;\n  value = value * 10 + 11;\n  value = value * 12 + 13;\n  value = value * 14 + 15;\n  value = value * 16 + 17;\n  value = value * 18 + 19;\n  return value;\n}\n";
    f.write("src/a.ts", source);
    f.write("src/b.ts", source);
    for selection in [vec![], vec!["--checks", "policy"]] {
        let mut args = vec![
            "check",
            "--engine",
            "mutation",
            "--max-diagnostics",
            "0",
            "--json",
        ];
        args.extend(&selection);
        let output = run(&f, &args);
        assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
        let report = json(&output);
        assert_eq!(report["accepted"], selection.is_empty());
        assert_eq!(report["partial"], !selection.is_empty());
        assert_eq!(report["shown"], 0);
        assert!(
            report["advisories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|note| note.as_str().unwrap().contains("clone"))
        );
    }
}
