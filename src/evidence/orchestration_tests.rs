use super::matches_baseline;

#[test]
fn only_the_exact_passing_executed_baseline_satisfies_a_test_command() {
    let baseline: Vec<String> = ["cargo", "test", "--workspace", "--locked"]
        .map(String::from)
        .into();
    let commands = vec![baseline.clone(), vec!["cargo".into(), "mutants".into()]];
    assert!(matches_baseline(true, &commands, &baseline));
    assert!(!matches_baseline(false, &commands, &baseline));
    let mut different = baseline.clone();
    different.push("--all-features".into());
    assert!(!matches_baseline(true, &commands, &different));
    assert!(!matches_baseline(
        true,
        std::slice::from_ref(&baseline),
        &baseline
    ));
    assert!(!matches_baseline(true, &commands, &[]));
}

#[test]
fn preflight_failures_are_global_and_disabled_producers_are_not_executed() {
    use super::*;
    let root = crate::fs_tests::tempdir("evidence-preflight");
    let mut context = ConfigContext::load_from(&root, None).unwrap();
    context.config.coverage.enabled = false;
    context.config.mutation.enabled = false;
    let runs = run_configured(&context, EvidenceMode::Cold);
    assert_eq!(runs.len(), 1);
    assert_eq!(
        (runs[0].kind, runs[0].name.as_str(), runs[0].status.as_str()),
        (None, "preflight", "failed")
    );
    context.config.mutation.enabled = true;
    let runs = run_configured(&context, EvidenceMode::Cold);
    assert!(runs[0].detail.as_ref().unwrap().contains("named"));
    for (name, producer) in [("disabled", "vitest"), ("missing", "cargo-mutants")] {
        context.config.evidence.producers.insert(
            name.into(),
            toml::from_str(&format!("producer='{producer}'\nsources=['src/*.rs']")).unwrap(),
        );
    }
    let runs = run_configured(&context, EvidenceMode::Cold);
    assert_eq!(runs.len(), 1);
    assert_eq!(
        (runs[0].kind, runs[0].name.as_str(), runs[0].status.as_str()),
        (Some(EvidenceKind::Mutation), "missing", "failed")
    );
    assert!(runs[0].report.ends_with("missing.json"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unauthenticated_or_failed_baselines_never_skip_an_identical_test_command() {
    use super::*;
    let root = crate::fs_tests::tempdir("untrusted-baseline");
    let context = ConfigContext::load_from(&root, None).unwrap();
    let report = root.join("mutation.json");
    let mut run = EvidenceRun {
        kind: Some(EvidenceKind::Mutation),
        name: "all".into(),
        status: "produced".into(),
        duration_ms: 1,
        report: report.clone(),
        detail: None,
    };
    assert!(baseline_for(&context, "cargo test", std::slice::from_ref(&run)).is_none());
    let sidecar = super::super::receipt_path(&report);
    let receipt = serde_json::json!({"schema_version":2,"root":root,"producer":"cargo-mutants","producer_version":"fixture","command":[["cargo","test"],["cargo","mutants"]],"runner_exit":0,"inputs":{},"report_sha256":"untrusted","restoration_verified":true,"prerequisite_passed":true,"workspace":null,"partition":null,"runtime_inputs":null});
    std::fs::write(&sidecar, receipt.to_string()).unwrap();
    for status in ["produced", "reused", "failed"] {
        run.status = status.into();
        assert!(baseline_for(&context, "cargo test", std::slice::from_ref(&run)).is_none());
        assert!(
            baseline_for(
                &context,
                "cargo test --all-features",
                std::slice::from_ref(&run)
            )
            .is_none()
        );
    }
    run.status = "produced".into();
    std::fs::write(&sidecar, "invalid JSON").unwrap();
    assert!(baseline_for(&context, "cargo test", &[run]).is_none());
    std::fs::remove_dir_all(root).unwrap();
}
