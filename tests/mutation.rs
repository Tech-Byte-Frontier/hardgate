#[path = "support/fs.rs"]
mod fs;

#[path = "support/mutations.rs"]
mod mutations;

use fs::tempdir;
use hardgate::config::MutationConfig;
use hardgate::engines::{AstMutationGenerator, MutationGatekeeper, MutationStats};
use mutations::has_mutation;
use std::path::{Path, PathBuf};

fn gatekeeper() -> MutationGatekeeper {
    gatekeeper_with_floor(85.0)
}

fn gatekeeper_with_floor(min_score: f64) -> MutationGatekeeper {
    MutationGatekeeper::new(&MutationConfig {
        enabled: true,
        min_score: Some(min_score),
        reject_timeouts: false,
        reports: None,
        test_cmd: None,
        timeout_secs: Some(10),
        max_mutants: Some(30),
    })
}

fn write_report(root: &Path, name: &str, content: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, content).unwrap();
    path
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
    let stryker = write_report(
        &tmp,
        "stryker.json",
        r#"{"files": {"a.rs": {"mutants": [{"status": "Killed"}, {"status": "Survived"}]}}}"#,
    );
    let low = keeper.evaluate_report(&stryker).unwrap();
    assert!(low.iter().any(|x| x.metric == "Mutation Kill Rate"));

    // cargo-mutants shape: everything caught = 100%, no violation.
    let caught = write_report(
        &tmp,
        "cm.json",
        r#"{"outcomes": [{"summary": "caught"}, {"summary": "caught"}]}"#,
    );
    assert!(keeper.evaluate_report(&caught).unwrap().is_empty());

    // Generic tallies behave the same at 90%+.
    let generic = write_report(
        &tmp,
        "gen.json",
        r#"{"killed": 9, "survived": 1, "timeout": 0}"#,
    );
    assert!(keeper.evaluate_report(&generic).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_zero_viable_score_is_zero() {
    assert_eq!(MutationStats::default().score_percent(), 0.0);

    let stats = MutationStats {
        unviable: 2,
        equivalent: 1,
        total: 3,
        ..Default::default()
    };
    assert_eq!(stats.score_percent(), 0.0);

    let overflowing_viable = MutationStats {
        killed: usize::MAX,
        survived: 1,
        ..Default::default()
    };
    assert_eq!(overflowing_viable.score_percent(), 0.0);
}

#[test]
fn test_mutation_report_rejects_malformed_and_empty_shapes() {
    let tmp = tempdir("mut-invalid");
    let keeper = gatekeeper();
    for (name, content) in [
        ("empty.json", ""),
        ("object.json", "{}"),
        ("stryker-empty.json", r#"{"files": {}}"#),
        (
            "stryker-mutants-empty.json",
            r#"{"files": {"a.rs": {"mutants": []}}}"#,
        ),
        ("cargo-empty.json", r#"{"outcomes": []}"#),
        (
            "cargo-malformed.json",
            r#"{"outcomes": [{"summary": "unknown"}]}"#,
        ),
        (
            "stryker-unknown-status.json",
            r#"{"files": {"a.rs": {"mutants": [{"status": "mystery"}]}}}"#,
        ),
        ("generic-malformed.json", r#"{"killed": "one"}"#),
        (
            "generic-unknown-outcome.json",
            r#"{"killed": 1, "mystery": 1}"#,
        ),
        (
            "generic-unknown-status.json",
            r#"{"killed": 1, "status": "mystery"}"#,
        ),
    ] {
        let path = write_report(&tmp, name, content);
        assert!(
            keeper.evaluate_report(&path).is_err(),
            "{name} should be rejected"
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_mutation_report_integrity_outcomes_are_blocking() {
    let tmp = tempdir("mut-integrity");
    let keeper = gatekeeper();
    let report = write_report(
        &tmp,
        "stryker.json",
        r#"{"files":{"a.rs":{"mutants":[
            {"status":"Killed"},
            {"status":"CompileError"},
            {"status":"RuntimeError"},
            {"status":"Equivalent"},
            {"status":"NoCoverage"}
        ]}}}"#,
    );
    let violations = keeper.evaluate_report(&report).unwrap();
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Mutation Compile Errors")
    );
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Mutation Runner Errors")
    );
    assert!(
        violations
            .iter()
            .any(|v| v.metric == "Mutation Unviable Mutants")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_timeout_is_blocking_even_when_legacy_flag_is_false() {
    let tmp = tempdir("mut-timeout");
    let keeper = gatekeeper();
    let report = write_report(&tmp, "timeout.json", r#"{"killed": 1, "timeout": 1}"#);

    let violations = keeper.evaluate_report(&report).unwrap();
    let timeout = violations
        .iter()
        .find(|violation| violation.metric == "Mutation Timeouts")
        .expect("timeout must be reported");
    assert_eq!(timeout.actual, 1.0);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_equivalent_is_excluded_from_score_but_reported() {
    let tmp = tempdir("mut-equivalent");
    let keeper = gatekeeper();

    let mixed = write_report(
        &tmp,
        "mixed.json",
        r#"{"killed": 9, "survived": 1, "equivalent": 100}"#,
    );
    assert!(keeper.evaluate_report(&mixed).unwrap().is_empty());

    let equivalent_only = write_report(&tmp, "only-equivalent.json", r#"{"equivalent": 2}"#);
    let violations = keeper.evaluate_report(&equivalent_only).unwrap();
    let score = violations
        .iter()
        .find(|violation| violation.metric == "Mutation Kill Rate")
        .expect("zero viable reports must fail the score floor");
    assert_eq!(score.actual, 0.0);
    assert!(score.message.contains("Equivalent: 2"));

    let zero_floor_keeper = gatekeeper_with_floor(0.0);
    assert!(
        !zero_floor_keeper
            .evaluate_report(&equivalent_only)
            .unwrap()
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_generic_counts_reject_overflow_and_mismatched_total() {
    let tmp = tempdir("mut-counts");
    let keeper = gatekeeper();

    let overflow = write_report(
        &tmp,
        "overflow.json",
        &format!(r#"{{"killed": {}, "survived": 1}}"#, usize::MAX),
    );
    assert!(keeper.evaluate_report(&overflow).is_err());

    let mismatch = write_report(
        &tmp,
        "mismatch.json",
        r#"{"killed": 1, "survived": 1, "total": 3}"#,
    );
    assert!(keeper.evaluate_report(&mismatch).is_err());

    let stryker_mismatch = write_report(
        &tmp,
        "stryker-mismatch.json",
        r#"{"files":{"a.rs":{"total":2,"mutants":[{"status":"Killed"}]}}}"#,
    );
    assert!(keeper.evaluate_report(&stryker_mismatch).is_err());

    let cargo_mismatch = write_report(
        &tmp,
        "cargo-mismatch.json",
        r#"{"total":2,"outcomes":[{"summary":"caught"}]}"#,
    );
    assert!(keeper.evaluate_report(&cargo_mismatch).is_err());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_cargo_mutants_integrity_statuses_and_declared_total() {
    let tmp = tempdir("mut-cargo-integrity");
    let keeper = gatekeeper();
    let report = write_report(
        &tmp,
        "cargo.json",
        r#"{"total": 7, "outcomes":[
            {"summary":"caught"},
            {"summary":"missed"},
            {"summary":"timeout"},
            {"summary":"compile_error"},
            {"summary":"error"},
            {"summary":"equivalent"},
            {"summary":"unviable"}
        ]}"#,
    );
    let violations = keeper.evaluate_report(&report).unwrap();
    for metric in [
        "Mutation Kill Rate",
        "Mutation Timeouts",
        "Mutation Compile Errors",
        "Mutation Runner Errors",
        "Mutation Unviable Mutants",
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.metric == metric),
            "missing {metric}"
        );
    }
    assert!(
        !violations
            .iter()
            .any(|violation| violation.metric == "Mutation Equivalent Mutants")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
