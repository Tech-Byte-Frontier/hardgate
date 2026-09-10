use super::{Project, lcov};
use serde_json::Value;

fn fixture() -> (Project, String) {
    let project = Project::new();
    std::fs::remove_file(project.0.join("src/lib.rs")).unwrap();
    std::fs::write(
        project.0.join("src/lib.js"),
        "export function answer() { return 42; }\n",
    )
    .unwrap();
    std::fs::write(project.0.join("hardgate.toml"), "[gate]\npreset='balanced'\n[coverage]\nenabled=true\n[evidence.producers.all]\nproducer='vitest'\nsources=['src/lib.js','index.js']\n").unwrap();
    let report = format!(
        "{}{}",
        lcov().replace("src/lib.rs", "src/lib.js"),
        lcov().replace("src/lib.rs", "index.js")
    );
    (project, report)
}

fn run(project: &Project, report: &str, mode: &str, semantic: &str) -> Value {
    let output = project
        .command()
        .args(["check", "--checks", "policy", "--json", "--evidence", mode])
        .env("HARDGATE_FIXTURE_REPORT", report)
        .env("HARDGATE_FIXTURE_MODE", "pass")
        .env("HARDGATE_FIXTURE_EXIT", "0")
        .env("SEMANTIC_CONFIGURATION", semantic)
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn status(report: &Value) -> &str {
    report["evidence_runs"][0]["status"]
        .as_str()
        .unwrap_or_else(|| panic!("missing attempt: {report}"))
}

#[test]
fn cold_then_warm_reuses_only_identical_environment_and_dependency_bytes() {
    let (project, report) = fixture();
    let cold = run(&project, &report, "cold", "one");
    assert_eq!(status(&cold), "produced", "{cold}");
    let warm = run(&project, &report, "reuse", "one");
    assert_eq!(status(&warm), "reused", "{warm}");
    let changed = run(&project, &report, "reuse", "two");
    assert_eq!(status(&changed), "produced", "{changed}");
    std::fs::write(
        project.0.join("node_modules/new-dependency.js"),
        "export const changed = true;",
    )
    .unwrap();
    let changed = run(&project, &report, "reuse", "two");
    assert_eq!(status(&changed), "produced", "{changed}");
    let forced = run(&project, &report, "cold", "two");
    assert_eq!(status(&forced), "produced", "{forced}");
    assert_eq!(
        forced["accepted"], false,
        "partial policy checks stay partial"
    );
}

#[test]
fn changed_tests_config_or_lockfiles_force_cold_production() {
    let (project, report) = fixture();
    assert_eq!(status(&run(&project, &report, "cold", "same")), "produced");
    for path in ["fixture.test.js", "vitest.config.mjs", "pnpm-lock.yaml"] {
        std::fs::write(project.0.join(path), "// changed relevant input").unwrap();
        let result = run(&project, &report, "reuse", "same");
        assert_eq!(status(&result), "produced", "{path}: {result}");
    }
}

#[test]
fn editing_both_report_and_sidecar_cannot_forge_warm_evidence() {
    use sha2::{Digest, Sha256};
    let (project, report) = fixture();
    assert_eq!(status(&run(&project, &report, "cold", "same")), "produced");
    let path = project.0.join(".hardgate/evidence/all.lcov");
    let sidecar = project.0.join(".hardgate/evidence/all.lcov.hardgate.json");
    let changed = std::fs::read_to_string(&path)
        .unwrap()
        .replace("DA:1,1", "DA:1,2");
    std::fs::write(&path, &changed).unwrap();
    let mut receipt: Value = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
    receipt["report_sha256"] = Value::String(
        Sha256::digest(changed.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    );
    std::fs::write(sidecar, serde_json::to_vec(&receipt).unwrap()).unwrap();
    let result = run(&project, &report, "reuse", "same");
    assert_eq!(status(&result), "produced", "{result}");
}

#[test]
fn external_node_preloads_always_run_cold() {
    let (project, report) = fixture();
    let preload = project.0.with_extension("preload.cjs");
    std::fs::write(&preload, "// external runtime input\n").unwrap();
    for _ in 0..2 {
        let output = project
            .command()
            .args([
                "check",
                "--checks",
                "policy",
                "--json",
                "--evidence",
                "reuse",
            ])
            .env("HARDGATE_FIXTURE_REPORT", &report)
            .env("NODE_OPTIONS", format!("--require {}", preload.display()))
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(status(&value), "produced", "{value}");
        assert!(
            value["evidence_runs"][0]["detail"]
                .as_str()
                .unwrap()
                .contains("external tool overrides")
        );
    }
    std::fs::remove_file(preload).unwrap();
}

#[test]
fn coverage_without_an_identical_baseline_cannot_skip_a_required_failing_test() {
    let (project, report) = fixture();
    let config = project.0.join("hardgate.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        format!("{text}\n[orchestration]\ntest_cmd = 'sh -c \"exit 17\"'\n"),
    )
    .unwrap();
    let output = project
        .command()
        .args(["check", "--checks", "tests", "--evidence", "cold", "--json"])
        .env("HARDGATE_FIXTURE_REPORT", report)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status(&value), "produced", "{value}");
    assert_eq!(value["accepted"], false);
    assert!(
        value["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|failure| failure["step"] == "test"),
        "{value}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout)
            .contains("Reused identical authenticated test baseline")
    );
}
