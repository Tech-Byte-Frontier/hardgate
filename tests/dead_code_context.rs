#[path = "common/cli.rs"]
mod cli;
#[path = "common/fs_git.rs"]
mod fs_git;

use cli::{Fixture, assert_status, json, run};
use fs_git::{commit_baseline, init_repo, write};
use hardgate::config::DeadCodeConfig;
use hardgate::engines::DeadCodeAnalyzer;
use std::path::{Path, PathBuf};

const CONFIG: &str =
    "[gate]\npreset = 'custom'\nstrict = true\n\n[analysis.dead_code]\nenabled = true\n";

fn fixture(tag: &str) -> Fixture {
    let fixture = Fixture::new("dead-code-context", tag, Some(CONFIG));
    write(
        &fixture,
        "src/shared.ts",
        "export function chosen() { return 1; }\n",
    );
    fixture.write(
        "src/index.ts",
        "import { chosen } from './shared';\nconsole.log(chosen());\n",
    );
    init_repo(&fixture);
    commit_baseline(&fixture, "baseline");
    fixture.write("src/shared.ts", "export function chosen() { return 2; }\n");
    fixture
}

#[test]
fn unchanged_importers_supply_context_for_changed_and_explicit_scopes() {
    let fixture = fixture("used");
    for args in [
        vec!["check", "--json"],
        vec!["check", "--diff", "--json"],
        vec!["check", "--json", "src/shared.ts"],
        vec!["verify", "--json", "src/shared.ts"],
    ] {
        let output = run(&fixture, &args);
        assert_status(&output, true, "unchanged importer context");
        assert!(
            json(&output)["dead_code_violations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    assert_status(
        &run(&fixture.join("src"), &["check", "--diff", "--json"]),
        true,
        "nested changed scope",
    );
}

#[test]
fn scoped_findings_exclude_unselected_debt_but_keep_truly_unused_exports() {
    let fixture = fixture("unused");
    fixture.write("src/orphan.ts", "export const unrelatedDebt = 3;\n");
    commit_baseline(&fixture, "unrelated existing debt");
    fixture.write(
        "src/shared.ts",
        "export function chosen() { return 4; }\nexport const neglected = 5;\n",
    );
    for args in [
        vec!["check", "--diff", "--json"],
        vec!["verify", "--json", "src/shared.ts"],
    ] {
        let output = run(&fixture, &args);
        assert_status(&output, false, "unused changed export");
        let report = json(&output);
        let findings = report["dead_code_violations"].as_array().unwrap();
        assert_eq!(findings.len(), 1, "{report}");
        assert_eq!(findings[0]["symbol"], "neglected");
        assert_eq!(findings[0]["file"], "src/shared.ts");
    }
    fixture.write("src/shared.ts", "export function chosen() { return 6; }\n");
    assert_status(
        &run(&fixture, &["check", "--json", "src/shared.ts"]),
        true,
        "unselected debt",
    );
    assert_status(
        &run(&fixture, &["check", "--json"]),
        false,
        "full debt remains visible",
    );
}

#[test]
fn unreadable_reference_context_cannot_produce_a_successful_scoped_verdict() {
    let fixture = fixture("read-error");
    std::fs::write(fixture.join("src/index.ts"), [0xff]).unwrap();
    let output = run(&fixture, &["check", "--json", "src/shared.ts"]);
    assert_status(&output, false, "incomplete reference context");
    let report = json(&output);
    assert!(
        report["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["step"] == "dead-code-context")
    );
}

#[test]
fn reference_index_preserves_word_boundaries_and_same_file_semantics() {
    let analyzer = DeadCodeAnalyzer::new(&DeadCodeConfig::default());
    for (reference, used) in [
        ("chosen()", true),
        ("chosenLong()", false),
        ("unchosen()", false),
        ("échosen", false),
        ("chosené", false),
        ("// chosen", true),
        ("'chosen'", true),
    ] {
        let contents = vec![
            (
                PathBuf::from("src/shared.ts"),
                "export function chosen() { return chosen; }".to_string(),
            ),
            (PathBuf::from("src/index.ts"), reference.to_string()),
        ];
        let files = contents
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        let findings = analyzer.analyze(&files, &contents, Path::new("."));
        let unused = findings
            .iter()
            .any(|finding| finding.symbol.as_deref() == Some("chosen"));
        assert_eq!(unused, !used, "{reference}");
    }
}
