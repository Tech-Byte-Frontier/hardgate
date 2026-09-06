use super::*;

fn diagnostic() -> Value {
    json!({"reason":"compiler-message","package_id":"fixture 0.1.0","target":{"name":"fixture","kind":["lib"]},"message":{
        "level":"warning","message":"review this return","code":{"code":"clippy::needless_return"},
        "spans":[{"is_primary":true,"file_name":"src/lib.rs","line_start":1,"column_start":1,"line_end":1}]
    }})
}

fn check(project: &Project, records: &[Value], exit: i32) -> Value {
    let stream = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let output = project
        .command()
        .args(["check", "--checks", "lint", "--json"])
        .env("HARDGATE_FIXTURE_REPORT", stream)
        .env("HARDGATE_FIXTURE_EXIT", exit.to_string())
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

fn finished(success: bool) -> Value {
    json!({"reason":"build-finished","success":success})
}

#[test]
fn cargo_target_duplicates_merge_but_different_diagnostics_remain_distinct() {
    let project = Project::new();
    std::fs::write(project.0.join("hardgate.toml"), "[gate]\npreset='balanced'\n[orchestration]\nlint='cargo clippy --message-format short --message-format=human -- -D warnings'\n").unwrap();
    let base = diagnostic();
    let mut other_target = base.clone();
    other_target["target"] = json!({"name":"integration","kind":["test"]});
    let merged = check(
        &project,
        &[
            base.clone(),
            other_target.clone(),
            other_target,
            finished(true),
        ],
        0,
    );
    assert_eq!(merged["summary"]["analysis_blockers"], 0, "{merged}");
    let findings = merged["tool_diagnostics"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["targets"].as_array().unwrap().len(), 2);
    for (pointer, value) in [
        ("/package_id", json!("different package")),
        ("/message/spans/0/file_name", json!("other.rs")),
        ("/message/spans/0/line_start", json!(2)),
        ("/message/spans/0/column_start", json!(2)),
        ("/message/message", json!("different warning")),
        ("/message/code/code", json!("clippy::different")),
        ("/message/level", json!("error")),
    ] {
        let mut distinct = base.clone();
        *distinct.pointer_mut(pointer).unwrap() = value;
        let error = distinct["message"]["level"] == "error";
        let report = check(
            &project,
            &[base.clone(), distinct, finished(!error)],
            i32::from(error),
        );
        assert_eq!(
            report["summary"]["analysis_blockers"], 0,
            "{pointer}: {report}"
        );
        assert_eq!(
            report["tool_diagnostics"].as_array().unwrap().len(),
            2,
            "{pointer}: {report}"
        );
    }
}

#[test]
fn incomplete_or_contradictory_cargo_streams_cannot_become_successful_lint_checks() {
    let project = Project::new();
    std::fs::write(
        project.0.join("hardgate.toml"),
        "[gate]\npreset='balanced'\n[orchestration]\nlint='cargo clippy --locked'\n",
    )
    .unwrap();
    let baseline = check(&project, &[finished(true)], 0);
    assert_eq!(baseline["passed"], true, "{baseline}");
    for (records, exit) in [
        (vec![], 0),
        (vec![finished(false)], 0),
        (vec![finished(true)], 1),
    ] {
        let report = check(&project, &records, exit);
        assert!(
            report["summary"]["analysis_blockers"].as_u64().unwrap() > 0,
            "{report}"
        );
    }
    let malformed = project
        .command()
        .args(["check", "--checks", "lint", "--json"])
        .env(
            "HARDGATE_FIXTURE_REPORT",
            "{broken json}\n{\"reason\":\"build-finished\",\"success\":true}\n",
        )
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&malformed.stdout).unwrap();
    assert!(
        report["summary"]["analysis_blockers"].as_u64().unwrap() > 0,
        "{report}"
    );
    for pointer in [
        "/message/level",
        "/message/spans",
        "/message/spans/0/file_name",
    ] {
        let mut unlocated = diagnostic();
        *unlocated.pointer_mut(pointer).unwrap() = Value::Null;
        let report = check(&project, &[unlocated, finished(true)], 0);
        assert_eq!(report["summary"]["analysis_blockers"], 0, "{report}");
        assert!(
            report["tool_diagnostics"].as_array().unwrap().is_empty(),
            "{report}"
        );
    }
    let mut rustc = diagnostic();
    rustc["message"]["code"] = Value::Null;
    rustc["message"]["level"] = json!("error");
    let report = check(&project, &[rustc, finished(false)], 1);
    assert_eq!(report["tool_diagnostics"][0]["tool"], "rustc", "{report}");
    assert_eq!(report["tool_diagnostics"][0]["rule"], "rustc");
}
