use super::*;

#[test]
fn redirected_or_nonexecuting_scope_options_cannot_issue_evidence() {
    let project = Project::new();
    for arguments in [
        vec!["--no-run"],
        vec!["--target-dir", "elsewhere"],
        vec!["--features"],
        vec!["--features="],
        vec!["--features", "--lib"],
        vec!["--all-features=true"],
    ] {
        let output = project
            .producer_command("cargo-mutants", &mutation(true).to_string(), ("pass", 0))
            .arg("--")
            .args(arguments)
            .output()
            .unwrap();
        assert_exit(&output, 2);
        assert!(!project.receipt("mutation").exists());
    }
    std::fs::create_dir(project.0.join(".cargo")).unwrap();
    for configuration in [
        "test_tool='nextest'",
        "additional_cargo_args='--lib'",
        "additional_cargo_test_args=[1]",
        "additional_cargo_args=['--no-run']",
    ] {
        std::fs::write(project.0.join(".cargo/mutants.toml"), configuration).unwrap();
        assert_exit(
            &project.produce("cargo-mutants", &mutation(true).to_string(), ("pass", 0)),
            2,
        );
    }
    let output = project
        .producer_command("vitest", lcov(), ("pass", 0))
        .args(["--", "--passWithNoTests"])
        .output()
        .unwrap();
    assert_exit(&output, 2);
    let output = project
        .producer_command("vitest", lcov(), ("pass", 0))
        .args(["--toolchain", "stable"])
        .output()
        .unwrap();
    assert_exit(&output, 2);
}

#[test]
fn published_receipt_identity_and_report_bytes_are_verified_independently() {
    use hardgate::evidence::EvidenceKind::Mutation;
    let project = Project::new();
    if project.nested_mutation_is_rejected("cargo-mutants") {
        return;
    }
    assert_exit(
        &project.produce("cargo-mutants", &mutation(true).to_string(), ("pass", 0)),
        0,
    );
    let path = project.receipt("mutation");
    let original_bytes = std::fs::read(&path).unwrap();
    let original: Value = serde_json::from_slice(&original_bytes).unwrap();
    for (key, value) in [
        ("schema_version", json!(1)),
        ("restoration_verified", json!(false)),
        ("producer", json!("vitest")),
        ("producer_version", json!("")),
        ("command", json!([])),
        ("command", json!([["cargo", "test", "--workspace"]])),
        (
            "command",
            json!([["cargo", "check", "--workspace"], ["cargo", "mutants"]]),
        ),
        ("command", json!([["cargo", "test"], ["cargo", "mutants"]])),
        ("runner_exit", json!(9)),
        ("report_sha256", json!("wrong")),
        ("inputs", json!({})),
    ] {
        let mut changed = original.clone();
        changed[key] = value;
        std::fs::write(&path, changed.to_string()).unwrap();
        assert!(project.verify(Mutation).is_err(), "accepted tampered {key}");
    }
    std::fs::write(&path, original_bytes).unwrap();
    std::fs::write(project.report("mutation"), mutation(false).to_string()).unwrap();
    assert!(format!("{:#}", project.verify(Mutation).unwrap_err()).contains("report bytes"));
}

#[test]
fn output_names_symlinks_and_missing_tools_cannot_publish_or_escape() {
    let project = Project::new();
    for name in ["../escape", "", "name.lcov"] {
        let output = project
            .producer_command("vitest", lcov(), ("pass", 0))
            .args(["--name", name])
            .output()
            .unwrap();
        assert_exit(&output, 2);
    }
    std::fs::remove_file(project.0.join("node_modules/.bin/vitest")).unwrap();
    assert_exit(&project.produce("vitest", lcov(), ("pass", 0)), 2);
    let directory = project.0.join(".hardgate/evidence");
    std::fs::remove_dir(&directory).unwrap();
    std::os::unix::fs::symlink(project.0.join("src"), &directory).unwrap();
    assert_exit(
        &project.produce("cargo-mutants", &mutation(true).to_string(), ("pass", 0)),
        2,
    );
    assert!(!project.0.join("src/mutation.json").exists());
}

#[test]
fn snapshots_preserve_internal_symlinks_but_reject_unbound_targets_and_special_files() {
    let project = Project::new();
    std::os::unix::fs::symlink("src/lib.rs", project.0.join("linked.rs")).unwrap();
    std::os::unix::fs::symlink("src", project.0.join("linked_directory")).unwrap();
    assert_exit(&project.produce("vitest", lcov(), ("pass", 0)), 0);
    project
        .verify(hardgate::evidence::EvidenceKind::Coverage)
        .unwrap();
    assert!(
        std::fs::symlink_metadata(project.0.join("linked.rs"))
            .unwrap()
            .is_symlink()
    );
    for target in [
        project.0.join("target"),
        std::path::PathBuf::from("/etc"),
        project.0.join("missing"),
    ] {
        if target.ends_with("target") {
            std::fs::create_dir(&target).unwrap();
        }
        let link = project.0.join("unbound");
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert_exit(&project.produce("vitest", lcov(), ("pass", 0)), 2);
        assert!(!project.receipt("coverage").exists());
        std::fs::remove_file(link).unwrap();
    }
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        project.0.join("pipe"),
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    let output = project.produce("vitest", lcov(), ("pass", 0));
    assert_exit(&output, 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported special file"));
}

#[test]
fn producer_temp_location_and_artifact_symlinks_cannot_redirect_evidence() {
    let project = Project::new();
    let output = project
        .producer_command("vitest", lcov(), ("pass", 0))
        .env_remove("HARDGATE_SCRATCH_ROOT")
        .env("TMPDIR", &project.0)
        .output()
        .unwrap();
    assert_exit(&output, 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside the source workspace"));
    let original = project.0.join("src/lib.rs");
    std::os::unix::fs::symlink(&original, project.report("coverage")).unwrap();
    assert_exit(&project.produce("vitest", lcov(), ("pass", 0)), 2);
    assert_eq!(std::fs::read_to_string(original).unwrap(), SOURCE);
}

#[test]
fn invalid_stryker_setup_and_unremovable_receipts_fail_before_execution() {
    let project = Project::new();
    if project.nested_mutation_is_rejected("stryker") {
        return;
    }
    std::fs::remove_file(project.0.join("stryker.config.json")).unwrap();
    let output = project.produce("stryker", "{}", ("pass", 0));
    assert_exit(&output, 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("exactly one"));
    let output = project
        .producer_command("stryker", "{}", ("pass", 0))
        .args(["--toolchain", "stable"])
        .output()
        .unwrap();
    assert_exit(&output, 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("configure Stryker scope"));
    std::fs::create_dir(project.receipt("coverage")).unwrap();
    assert_exit(&project.produce("vitest", lcov(), ("pass", 0)), 2);
    assert!(!project.report("coverage").exists());
}

#[test]
fn copied_report_and_receipt_cannot_authenticate_another_artifact_path() {
    let project = Project::new();
    assert_exit(&project.produce("vitest", lcov(), ("pass", 0)), 0);
    let moved = project.0.join("moved.lcov");
    std::fs::copy(project.report("coverage"), &moved).unwrap();
    std::fs::copy(
        project.receipt("coverage"),
        project.0.join("moved.lcov.hardgate.json"),
    )
    .unwrap();
    let error = hardgate::evidence::verify(
        &project.0,
        &moved,
        hardgate::evidence::EvidenceKind::Coverage,
        &Default::default(),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("protected execution authentication"));
}

#[test]
fn protected_producer_cannot_write_parent_execution_authentication() {
    use sha2::{Digest, Sha256};
    let project = Project::new();
    assert_exit(&project.produce("vitest", lcov(), ("pass", 0)), 0);
    let receipt = std::fs::read(project.receipt("coverage")).unwrap();
    let digest: String = Sha256::digest(&receipt)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/state")
        });
    let record = state.join("hardgate/executions-v2").join(digest);
    assert_eq!(std::fs::read(&record).unwrap(), receipt);
    let output = project
        .producer_command("vitest", lcov(), ("forge-authentication", 0))
        .env("HARDGATE_FIXTURE_AUTHENTICATION_TARGET", &record)
        .output()
        .unwrap();
    assert_exit(&output, 2);
    assert_eq!(std::fs::read(record).unwrap(), receipt);
    assert!(!project.receipt("coverage").exists());
}
