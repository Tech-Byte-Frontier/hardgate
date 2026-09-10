use super::{Project, assert_exit, lcov};
use hardgate::{
    config::ConfigContext,
    evidence::{EvidenceKind, reports, verify_set},
};

fn config(project: &Project, second_sources: &str) -> ConfigContext {
    std::fs::remove_file(project.0.join("src/lib.rs")).ok();
    std::fs::write(
        project.0.join("src/lib.js"),
        "export function answer() { return 42; }\n",
    )
    .unwrap();
    std::fs::write(project.0.join("hardgate.toml"), format!("[gate]\npreset='balanced'\n[coverage]\nenabled=true\n[evidence.producers.rust]\nproducer='vitest'\nsources=['src/lib.js']\n[evidence.producers.js]\nproducer='vitest'\nsources=[{second_sources}]\n")).unwrap();
    ConfigContext::load_from(&project.0, None).unwrap()
}

fn produce(project: &Project, name: &str, report: &str) -> std::process::Output {
    project
        .producer_command(
            "vitest",
            &report.replace("src/lib.rs", "src/lib.js"),
            ("pass", 0),
        )
        .args(["--producer-config", name])
        .output()
        .unwrap()
}

fn verify(context: &ConfigContext) -> anyhow::Result<()> {
    verify_set(
        &context.root,
        &reports(&context.config, EvidenceKind::Coverage),
        EvidenceKind::Coverage,
        &context.config,
    )
}

#[test]
fn named_coverage_requires_all_disjoint_source_bound_partitions() {
    let project = Project::new();
    let context = config(&project, "'index.js'");
    assert_exit(&produce(&project, "rust", lcov()), 0);
    assert!(
        verify(&context).is_err(),
        "missing named producer must block"
    );
    let js = lcov().replace("src/lib.rs", "index.js");
    assert_exit(&produce(&project, "js", &js), 0);
    verify(&context).unwrap();
    let mut changed = context.config.clone();
    changed
        .evidence
        .producers
        .get_mut("rust")
        .unwrap()
        .timeout_secs += 1;
    let error = hardgate::evidence::verify(
        &project.0,
        &project.0.join(".hardgate/evidence/rust.lcov"),
        EvidenceKind::Coverage,
        &changed,
    )
    .unwrap_err();
    assert!(error.to_string().contains("producer configuration changed"));
    std::fs::write(project.0.join("new.ts"), "export const missing = 1;").unwrap();
    assert!(
        verify(&context).is_err(),
        "new executable source must invalidate evidence"
    );
}

#[test]
fn named_producer_rejects_missing_partition_records_and_stale_dependencies() {
    let project = Project::new();
    let context = config(&project, "'index.js', 'src/lib.js'");
    assert_exit(&produce(&project, "js", lcov()), 2);
    assert!(
        !project
            .0
            .join(".hardgate/evidence/js.lcov.hardgate.json")
            .exists()
    );
    let report = format!("{}{}", lcov(), lcov().replace("src/lib.rs", "index.js"));
    assert_exit(&produce(&project, "js", &report), 0);
    assert_exit(&produce(&project, "rust", lcov()), 0);
    assert!(
        verify(&context)
            .unwrap_err()
            .to_string()
            .contains("overlapping")
    );
    std::fs::write(
        project.0.join("pnpm-lock.yaml"),
        "changed dependency identity",
    )
    .unwrap();
    let result = hardgate::evidence::verify(
        &project.0,
        &project.0.join(".hardgate/evidence/js.lcov"),
        EvidenceKind::Coverage,
        &context.config,
    );
    assert!(result.unwrap_err().to_string().contains("inputs changed"));
}

#[test]
fn a_failed_rerun_revokes_the_old_named_receipt() {
    let project = Project::new();
    config(&project, "'index.js'");
    assert_exit(&produce(&project, "rust", lcov()), 0);
    let path = project.0.join(".hardgate/evidence/rust.lcov.hardgate.json");
    let old = std::fs::read(&path).unwrap();
    let output = project
        .producer_command(
            "vitest",
            &lcov().replace("src/lib.rs", "src/lib.js"),
            ("baseline-fail", 9),
        )
        .args(["--producer-config", "rust"])
        .output()
        .unwrap();
    assert_exit(&output, 2);
    assert!(
        !project
            .0
            .join(".hardgate/evidence/rust.lcov.hardgate.json")
            .exists()
    );
    std::fs::write(&path, old).unwrap();
    let context = ConfigContext::load_from(&project.0, None).unwrap();
    assert!(
        hardgate::evidence::verify(
            &project.0,
            &project.0.join(".hardgate/evidence/rust.lcov"),
            EvidenceKind::Coverage,
            &context.config
        )
        .is_err(),
        "restoring an old sidecar must not restore revoked authentication"
    );
}

#[test]
fn named_producers_reject_incompatible_executable_languages() {
    let project = Project::new();
    std::fs::write(project.0.join("hardgate.toml"), "[gate]\npreset='balanced'\n[evidence.producers.wrong]\nproducer='vitest'\nsources=['src/lib.rs']\n").unwrap();
    let output = project
        .producer_command("vitest", lcov(), ("pass", 0))
        .args(["--producer-config", "wrong"])
        .output()
        .unwrap();
    assert_exit(&output, 2);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot instrument executable source")
    );
    let unknown = project
        .producer_command("vitest", lcov(), ("pass", 0))
        .args(["--producer-config", "missing"])
        .output()
        .unwrap();
    assert_exit(&unknown, 2);
    assert!(
        String::from_utf8_lossy(&unknown.stderr)
            .contains("unknown evidence producer configuration")
    );
    assert!(!project.0.join(".hardgate/evidence").exists());
}

#[test]
fn named_rust_mutation_owns_file_scope_and_keeps_the_full_workspace_baseline() {
    let project = Project::new();
    if project.nested_mutation_is_rejected("cargo-mutants") {
        return;
    }
    std::fs::write(project.0.join("hardgate.toml"), "[gate]\npreset='balanced'\n[evidence.producers.rust]\nproducer='cargo-mutants'\nsources=['src/lib.rs']\n").unwrap();
    let output = project
        .producer_command(
            "cargo-mutants",
            &super::mutation(true).to_string(),
            ("pass", 0),
        )
        .args(["--producer-config", "rust"])
        // A custom compiler environment requires cold execution; the protocol
        // fixture does not depend on the host's installed Rust toolchains.
        .env("RUSTFLAGS", "--cfg protocol_fixture")
        .output()
        .unwrap();
    assert_exit(&output, 0);
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(project.0.join(".hardgate/evidence/rust.json.hardgate.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        receipt["partition"]["sources"],
        serde_json::json!(["src/lib.rs"])
    );
    assert_eq!(receipt["prerequisite_passed"], true);
    let baseline = receipt["command"][0].as_array().unwrap();
    assert!(baseline.iter().any(|arg| arg == "--workspace"));
    assert!(!baseline.iter().any(|arg| arg == "--file"));
    let command = receipt["command"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_array()
        .unwrap();
    assert!(
        command
            .windows(2)
            .any(|args| args == ["--file", "src/lib.rs"])
    );
    assert_eq!(receipt["runtime_inputs"], serde_json::Value::Null);
}
