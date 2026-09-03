#[path = "support/fs.rs"]
mod fs;

#[path = "support/mutations.rs"]
mod mutations;

use fs::tempdir;
use hardgate::config::MutationConfig;
use hardgate::engines::{AstMutationGenerator, MutationGatekeeper};
use mutations::has_mutation;
use std::path::Path;

fn gatekeeper() -> MutationGatekeeper {
    MutationGatekeeper::new(&MutationConfig {
        enabled: true,
        min_score: Some(85.0),
        reject_timeouts: false,
        reports: None,
        test_cmd: None,
        timeout_secs: Some(10),
        max_mutants: Some(30),
    })
}

#[test]
fn test_ast_mutation_generator() {
    let mut generator = AstMutationGenerator::new();

    let code = r#"
    fn evaluate(a: i32, b: i32) -> bool {
        if a == b && a > 0 {
            return true;
        }
        false
    }
    "#;

    let mutants = generator.generate_mutants(Path::new("src/calc.rs"), code);
    assert!(!mutants.is_empty());
    assert!(has_mutation(&mutants, "==", "!="));
    assert!(has_mutation(&mutants, "&&", "||"));
    assert!(has_mutation(&mutants, ">", "<="));
    assert!(has_mutation(&mutants, "true", "false"));
}

#[test]
fn test_mutation_report_parsers() {
    let tmp = tempdir("mut");
    let keeper = gatekeeper();

    // Stryker shape: 1 killed / 1 survived = 50% < 85% floor.
    let stryker = tmp.join("stryker.json");
    std::fs::write(
        &stryker,
        r#"{"files": {"a.rs": {"mutants": [{"status": "Killed"}, {"status": "Survived"}]}}}"#,
    )
    .unwrap();
    let low = keeper.evaluate_report(&stryker).unwrap();
    assert!(low.iter().any(|x| x.metric == "Mutation Kill Rate"));

    // cargo-mutants shape: everything caught = 100%, no violation.
    let caught = tmp.join("cm.json");
    std::fs::write(
        &caught,
        r#"{"outcomes": [{"summary": "caught"}, {"summary": "caught"}]}"#,
    )
    .unwrap();
    assert!(keeper.evaluate_report(&caught).unwrap().is_empty());

    // Generic tallies behave the same at 90%+.
    let generic = tmp.join("gen.json");
    std::fs::write(&generic, r#"{"killed": 9, "survived": 1, "timeout": 0}"#).unwrap();
    assert!(keeper.evaluate_report(&generic).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}
