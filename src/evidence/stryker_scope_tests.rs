use super::*;
use serde_json::{Value, json};

fn native() -> (Value, super::super::Snapshot) {
    let inputs = super::super::Snapshot(BTreeMap::from([
        (PathBuf::from("answer.js"), "native-answer-hash".into()),
        (PathBuf::from("reexport.js"), "native-reexport-hash".into()),
    ]));
    let report = json!({
        "framework": {"version":"10.0.0"},
        "files": {"answer.js": {"mutants":[{"id":"0","status":"Killed"}]}},
        "hardgate_scope": {
            "schema_version":1, "producer_version":"10.0.0",
            "sources":inputs.0, "mutants":[["answer.js","0"]],
            "baseline_passed":true, "completed":true
        }
    });
    (report, inputs)
}

#[test]
fn native_scope_includes_zero_mutant_sources_and_rejects_incomplete_plans() {
    let (report, inputs) = native();
    assert_eq!(
        sources(&report, &inputs).unwrap().unwrap(),
        inputs.0.keys().cloned().collect()
    );
    for (pointer, value) in [
        ("/hardgate_scope/completed", json!(false)),
        ("/hardgate_scope/baseline_passed", json!(false)),
        ("/hardgate_scope/schema_version", json!(2)),
        ("/hardgate_scope/producer_version", json!("9.0.0")),
        ("/hardgate_scope/sources/answer.js", json!("wrong-source")),
        ("/hardgate_scope/mutants", json!([])),
        ("/files/answer.js/mutants", json!([])),
        ("/files/answer.js/mutants/0/id", Value::Null),
    ] {
        let mut changed = report.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(sources(&changed, &inputs).is_err(), "accepted {pointer}");
    }
    let mut changed = report.clone();
    changed["files"]["unplanned.js"] = json!({"mutants":[]});
    assert!(sources(&changed, &inputs).is_err());
    changed = report;
    changed["files"]["answer.js"]["mutants"] = json!([{"id":"0"},{"id":"0"}]);
    assert!(sources(&changed, &inputs).is_err());
}

#[test]
fn native_scope_sidecar_is_merged_once_and_reporter_quotes_its_destination() {
    let root = crate::fs_tests::tempdir("scope-reporter");
    let output = root.join("quoted \" directory");
    std::fs::create_dir(&output).unwrap();
    let reporter = prepare(&output).unwrap();
    let body = std::fs::read_to_string(reporter).unwrap();
    assert!(body.contains(&serde_json::to_string(&output.join("mutation.scope.json")).unwrap()));
    assert!(!body.contains("HARDGATE_SCOPE_DESTINATION"));
    let report = output.join("mutation.json");
    let (native, _) = native();
    let mut score = native.clone();
    score.as_object_mut().unwrap().remove("hardgate_scope");
    std::fs::write(
        report.with_extension("scope.json"),
        native["hardgate_scope"].to_string(),
    )
    .unwrap();
    let merged: Value =
        serde_json::from_slice(&merge(&score.to_string(), &report).unwrap()).unwrap();
    assert_eq!(merged, native);
    assert!(merge(&native.to_string(), &report).is_err());
    assert!(merge("invalid JSON", &report).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
