use super::*;

#[test]
fn coverage_producers_publish_normalized_source_bound_evidence_and_reject_staleness() {
    use hardgate::evidence::EvidenceKind::Coverage;
    for producer in ["cargo-llvm-cov", "vitest"] {
        let project = Project::new();
        assert_exit(&project.produce(producer, lcov(), ("pass", 0)), 0);
        project.verify(Coverage).unwrap();
        let checked = project.check_report("coverage");
        assert!(
            checked["execution"]["engines"]
                .as_array()
                .unwrap()
                .iter()
                .any(|engine| engine["id"] == "coverage" && engine["state"] == "failed"),
            "{checked}"
        );
        assert_eq!(
            checked["coverage_violations"][0]["metric"],
            "Global Branch Coverage"
        );
        assert_eq!(checked["coverage_violations"][0]["actual"], 0.0);
        assert!(
            std::fs::read_to_string(project.report("coverage"))
                .unwrap()
                .starts_with("SF:src/lib.rs\n")
        );
        let receipt: Value =
            serde_json::from_slice(&std::fs::read(project.receipt("coverage")).unwrap()).unwrap();
        assert_eq!(receipt["prerequisite_passed"], producer == "cargo-llvm-cov");
        assert_eq!(receipt["restoration_verified"], true);
        assert_eq!(
            receipt["command"].as_array().unwrap().len(),
            if producer == "cargo-llvm-cov" { 2 } else { 1 }
        );
        std::fs::write(project.0.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
        assert!(format!("{:#}", project.verify(Coverage).unwrap_err()).contains("stale evidence"));
    }
}

#[test]
fn mutation_receipts_preserve_survivors_and_require_workspace_baseline_identity() {
    use hardgate::evidence::EvidenceKind::Mutation;
    for (caught, exit) in [(true, 0), (false, 2), (false, 3)] {
        let project = Project::new();
        if project.nested_mutation_is_rejected("cargo-mutants") {
            return;
        }
        assert_exit(
            &project.produce(
                "cargo-mutants",
                &mutation(caught).to_string(),
                ("pass", exit),
            ),
            i32::from(exit != 0),
        );
        project.verify(Mutation).unwrap();
        let checked = project.check_report("mutation");
        assert_eq!(
            checked["passed"], false,
            "sample cannot establish full mutation scope: {checked}"
        );
        assert_eq!(checked["status"], "incomplete");
        assert_eq!(
            checked["mutation_violations"]
                .as_array()
                .unwrap()
                .is_empty(),
            caught,
            "sample score diagnostics remain available: {checked}"
        );
        assert_eq!(checked["partial"], true);
        let receipt_path = project.receipt("mutation");
        let mut receipt: Value =
            serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
        assert!(
            receipt["command"][0]
                .as_array()
                .unwrap()
                .contains(&json!("--workspace"))
        );
        assert_eq!(receipt["runner_exit"], exit);
        receipt["prerequisite_passed"] = json!(false);
        std::fs::write(&receipt_path, receipt.to_string()).unwrap();
        assert!(
            format!("{:#}", project.verify(Mutation).unwrap_err())
                .contains("protected execution authentication")
        );
        assert_eq!(
            std::fs::read_to_string(project.0.join("src/lib.rs")).unwrap(),
            SOURCE
        );
    }
}

#[test]
fn failed_producers_remove_prior_receipts_and_never_modify_original_source() {
    for mode in [
        "missing",
        "empty",
        "malformed",
        "version-fail",
        "version-empty",
        "baseline-fail",
        "unrestored",
        "timeout",
    ] {
        let project = Project::new();
        if project.nested_mutation_is_rejected("cargo-mutants") {
            return;
        }
        let report = mutation(true).to_string();
        assert_exit(&project.produce("cargo-mutants", &report, ("pass", 0)), 0);
        let failed = project.produce("cargo-mutants", &report, (mode, 0));
        assert_exit(&failed, 2);
        assert!(!project.receipt("mutation").exists(), "{mode}");
        assert!(
            !project
                .report("mutation")
                .with_extension("pending")
                .exists(),
            "{mode}"
        );
        assert_eq!(
            std::fs::read_to_string(project.0.join("src/lib.rs")).unwrap(),
            SOURCE
        );
    }
}

#[test]
fn stryker_overrides_unsafe_runner_defaults_and_binds_reported_source() {
    use hardgate::evidence::EvidenceKind::Mutation;
    for exit in [0, 1] {
        let project = Project::new();
        if project.nested_mutation_is_rejected("stryker") {
            return;
        }
        if exit == 1 {
            std::fs::remove_file(project.0.join("stryker.config.json")).unwrap();
            std::fs::write(
                project.0.join("stryker.config.mjs"),
                "export default { incremental: true };\n",
            )
            .unwrap();
        }
        let source = std::fs::read_to_string(project.0.join("index.js")).unwrap();
        let report =
            json!({"files":{"index.js":{"source":source,"mutants":[{"status":"Killed"}]}}});
        assert_exit(
            &project.produce("stryker", &report.to_string(), ("pass", exit)),
            exit,
        );
        project.verify(Mutation).unwrap();
        let wrong =
            json!({"files":{"index.js":{"source":"wrong bytes","mutants":[{"status":"Killed"}]}}});
        assert_exit(
            &project.produce("stryker", &wrong.to_string(), ("pass", 0)),
            2,
        );
        assert!(!project.receipt("mutation").exists());
    }
}

#[test]
fn stryker_respects_configured_concurrency_ceiling_and_rejects_zero() {
    let project = Project::new();
    if project.nested_mutation_is_rejected("stryker") {
        return;
    }
    let source = std::fs::read_to_string(project.0.join("index.js")).unwrap();
    let report = json!({"files":{"index.js":{"source":source,"mutants":[{"status":"Killed"}]}}});
    // One stays exactly one; two may be capped to one by live memory headroom.
    for concurrency in [1, 2] {
        std::fs::write(
            project.0.join("stryker.config.json"),
            json!({"concurrency":concurrency}).to_string(),
        )
        .unwrap();
        let result = project
            .producer_command("stryker", &report.to_string(), ("pass", 0))
            .env("HARDGATE_FIXTURE_CONCURRENCY", concurrency.to_string())
            .output()
            .unwrap();
        assert_exit(&result, 0);
        project
            .verify(hardgate::evidence::EvidenceKind::Mutation)
            .unwrap();
    }
    std::fs::write(project.0.join("stryker.config.json"), "{\"concurrency\":0}").unwrap();
    assert_exit(
        &project.produce("stryker", &report.to_string(), ("pass", 0)),
        2,
    );
    assert!(!project.receipt("mutation").exists());
}

#[test]
fn mutation_scope_and_feature_settings_reach_the_original_workspace_baseline() {
    let project = Project::new();
    if project.nested_mutation_is_rejected("cargo-mutants") {
        return;
    }
    std::fs::create_dir(project.0.join(".cargo")).unwrap();
    std::fs::write(project.0.join(".cargo/mutants.toml"), "all_features=true\nno_default_features=true\nfeatures=['fast']\nprofile='release'\nadditional_cargo_args=['--offline']\nadditional_cargo_test_args=['--lib']\n").unwrap();
    let output = project
        .producer_command("cargo-mutants", &mutation(true).to_string(), ("pass", 0))
        .args([
            "--",
            "--package=fixture",
            "--features",
            "cli",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--bin",
            "worker",
            "--test=integration",
            "--file",
            "src/lib.rs",
            "--re",
            "answer",
            "--shard",
            "0/1",
            "--timeout",
            "30",
            "--offline",
        ])
        .output()
        .unwrap();
    assert_exit(&output, 0);
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(project.receipt("mutation")).unwrap()).unwrap();
    let baseline = receipt["command"][0].as_array().unwrap();
    for argument in [
        "test",
        "--workspace",
        "--locked",
        "--features",
        "cli",
        "--all-features",
        "--no-default-features",
        "--features=fast",
        "--profile=release",
        "--target",
        "x86_64-unknown-linux-gnu",
        "--bin",
        "worker",
        "--test",
        "integration",
        "--lib",
        "--offline",
    ] {
        assert!(
            baseline.contains(&json!(argument)),
            "missing {argument}: {baseline:?}"
        );
    }
    for argument in ["--package", "--file", "--re", "--shard", "--timeout"] {
        assert!(
            !baseline.contains(&json!(argument)),
            "mutant selection narrowed baseline: {baseline:?}"
        );
    }
    project
        .verify(hardgate::evidence::EvidenceKind::Mutation)
        .unwrap();
}

#[test]
fn explicit_coverage_targets_keep_package_scope_and_omit_workspace_prerequisite() {
    for target in [
        "--lib",
        "--bins",
        "--tests",
        "--examples",
        "--benches",
        "--all-targets",
    ] {
        let project = Project::new();
        let output = project
            .producer_command("cargo-llvm-cov", lcov(), ("pass", 0))
            .args(["--", "--package=fixture", target, "--no-default-features"])
            .output()
            .unwrap();
        assert_exit(&output, 0);
        project
            .verify(hardgate::evidence::EvidenceKind::Coverage)
            .unwrap();
        let receipt: Value =
            serde_json::from_slice(&std::fs::read(project.receipt("coverage")).unwrap()).unwrap();
        assert_eq!(receipt["prerequisite_passed"], false);
        let commands = receipt["command"].as_array().unwrap();
        assert_eq!(commands.len(), 1);
        let args = commands[0].as_array().unwrap();
        for required in [
            "--package",
            "fixture",
            target,
            "--no-default-features",
            "--branch",
            "--include-build-script",
        ] {
            assert!(args.contains(&json!(required)), "{args:?}");
        }
        for absent in ["--workspace", "--doc", "--no-clean", "--no-report"] {
            assert!(!args.contains(&json!(absent)), "{args:?}");
        }
    }
}

#[test]
fn explicit_mutation_feature_flags_are_not_duplicated_by_project_configuration() {
    let project = Project::new();
    if project.nested_mutation_is_rejected("cargo-mutants") {
        return;
    }
    std::fs::create_dir(project.0.join(".cargo")).unwrap();
    for config in [
        "all_features=true\nno_default_features=true\ntest_tool='cargo'\n",
        "all_features=false\nno_default_features=false\n",
    ] {
        std::fs::write(project.0.join(".cargo/mutants.toml"), config).unwrap();
        let output = project
            .producer_command("cargo-mutants", &mutation(true).to_string(), ("pass", 0))
            .args([
                "--",
                "--all-features",
                "--no-default-features",
                "--build-timeout=30",
                "--example=demo",
                "--bench=timing",
            ])
            .output()
            .unwrap();
        assert_exit(&output, 0);
        project
            .verify(hardgate::evidence::EvidenceKind::Mutation)
            .unwrap();
        let receipt: Value =
            serde_json::from_slice(&std::fs::read(project.receipt("mutation")).unwrap()).unwrap();
        let baseline = receipt["command"][0].as_array().unwrap();
        for flag in [
            "--all-features",
            "--no-default-features",
            "--example",
            "demo",
            "--bench",
            "timing",
        ] {
            assert_eq!(
                baseline
                    .iter()
                    .filter(|value| **value == json!(flag))
                    .count(),
                1,
                "{baseline:?}"
            );
        }
        assert!(!baseline.contains(&json!("--build-timeout")));
    }
}

#[test]
fn pytest_protocol_requires_passing_tests_and_native_explicit_function_regions() {
    let project = Project::new();
    std::fs::create_dir_all(project.0.join(".venv/bin")).unwrap();
    std::fs::copy(
        project.0.join("tools/cargo"),
        project.0.join(".venv/bin/python"),
    )
    .unwrap();
    std::fs::write(
        project.0.join("src/calculation.py"),
        "def answer():\n    return 42\n",
    )
    .unwrap();
    let report = lcov().replace("FIXTURE_ROOT/src/lib.rs", "src/calculation.py");
    let native = json!({"meta":{"branch_coverage":true}, "files":{"src/calculation.py":{"summary":{"num_statements":1,"covered_lines":1,"num_branches":0,"covered_branches":0},"functions":{"answer":{"summary":{"num_statements":1,"covered_lines":1}}}}}});
    let run = |mode| {
        project
            .producer_command("pytest", &report, (mode, 0))
            .env("HARDGATE_FIXTURE_PYTHON_JSON", native.to_string())
            .output()
            .unwrap()
    };
    assert_exit(&run("pass"), 0);
    project
        .verify(hardgate::evidence::EvidenceKind::Coverage)
        .unwrap();
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(project.receipt("coverage")).unwrap()).unwrap();
    assert_eq!(receipt["prerequisite_passed"], true);
    assert_eq!(receipt["command"].as_array().unwrap().len(), 3);
    assert_exit(&run("baseline-fail"), 2);
    assert!(!project.receipt("coverage").exists());
}
