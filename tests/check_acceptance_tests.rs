#[path = "support/fs.rs"]
mod fs;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Project(PathBuf);

fn fixture_command(program: &str) -> Command {
    let mut command = Command::new(program);
    // Fixture projects exercise the supported stable specialists, even when
    // this test executable was built by the branch-coverage nightly.
    // The package MSRV is a compatibility floor, not the tested specialist
    // toolchain. CI supplies its pin; local runs use rust-toolchain.toml's pin.
    command.env(
        "RUSTUP_TOOLCHAIN",
        option_env!("RUST_TOOLCHAIN").unwrap_or("1.98.1"),
    );
    for variable in [
        "RUSTC",
        "RUSTDOC",
        "RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_ENCODED_RUSTDOCFLAGS",
    ] {
        command.env_remove(variable);
    }
    command
}

impl Project {
    fn new(name: &str) -> Self {
        let root = fs::tempdir(name);
        std::fs::write(root.join("index.js"), "export const answer = 42;\n").unwrap();
        Self(root)
    }

    fn policy(&self, orchestration: &str) {
        std::fs::write(
            self.0.join("hardgate.toml"),
            format!("[gate]\npreset = \"balanced\"\n[orchestration]\n{orchestration}\n"),
        )
        .unwrap();
    }

    fn check(&self, args: &[&str]) -> Value {
        let output = fixture_command(env!("CARGO_BIN_EXE_hardgate"))
            .args(["check", "--json"])
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap();
        let report: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{error}: {:?}", output));
        assert_eq!(
            output.status.code().unwrap() as u64,
            report["exit_code"].as_u64().unwrap()
        );
        report
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn checks_protect_original_bytes_from_relative_and_absolute_fix_commands() {
    let project = Project::new("check-write-guard");
    let original = std::fs::read(project.0.join("index.js")).unwrap();
    for destination in [
        "index.js".to_owned(),
        project.0.join("index.js").display().to_string(),
    ] {
        let command = format!("sh -c 'printf changed > {destination}'");
        project.policy(&format!(
            "format_check = {}",
            serde_json::to_string(&command).unwrap()
        ));
        let report = project.check(&["--checks", "format"]);
        assert_eq!(report["passed"], false, "{report}");
        assert_eq!(report["accepted"], false);
        assert_eq!(std::fs::read(project.0.join("index.js")).unwrap(), original);
    }
}

#[test]
fn default_acceptance_requires_format_and_lint_and_distinguishes_partial_success() {
    let project = Project::new("check-default-contract");
    project.policy("format_check = \"sh -c 'exit 0'\"\nlint = \"sh -c 'exit 0'\"");
    let complete = project.check(&[]);
    assert_eq!(complete["accepted"], true, "{complete}");
    let partial = project.check(&["--checks", "policy"]);
    assert_eq!(partial["passed"], true);
    assert_eq!(partial["accepted"], false);
    assert_eq!(partial["partial"], true);
    assert!(
        partial["omitted_requirements"]
            .as_array()
            .unwrap()
            .contains(&Value::from("lint"))
    );
    project.policy("format_check = \"sh -c 'exit 0'\"\nlint = \"missing-hardgate-fixture-tool\"");
    let missing = project.check(&[]);
    assert_eq!(missing["accepted"], false);
    assert!(missing["summary"]["analysis_blockers"].as_u64().unwrap() > 0);
}

fn rust_workspace(root: &Path, source: &str) {
    std::fs::create_dir_all(root.join("member/src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=[\"member\"]\nresolver=\"3\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("member/Cargo.toml"),
        "[package]\nname=\"check-member\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    std::fs::write(root.join("member/src/lib.rs"), source).unwrap();
    let output = fixture_command("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn workspace_member_and_doctest_failures_both_reach_the_gate() {
    let project = Project::new("check-workspace-tests");
    project.policy("");
    rust_workspace(
        &project.0,
        "/// ```\n/// assert_eq!(1, 2);\n/// ```\npub fn answer() -> u32 { 42 }\n#[cfg(test)]\nmod tests { #[test] fn member_failure() { assert_eq!(1, 2); } }\n",
    );
    let report = project.check(&["--checks", "tests"]);
    let failures = report["orchestration_violations"].as_array().unwrap();
    assert_eq!(failures.len(), 2, "{report}");
    assert!(failures.iter().any(|failure| {
        failure["output"]
            .as_str()
            .unwrap()
            .contains("member_failure")
    }));
    assert!(failures.iter().any(
        |failure| failure["command"].as_str().unwrap().contains("--doc")
            && failure["output"].as_str().unwrap().contains("doctest")
    ));
}

#[test]
fn real_clippy_findings_keep_rule_locations_and_target_without_execution_blocker() {
    let project = Project::new("check-clippy-diagnostics");
    project.policy("");
    rust_workspace(
        &project.0,
        "pub fn first() -> usize { return 1; }\npub fn second() -> usize { return 2; }\n",
    );
    let report = project.check(&["--checks", "lint"]);
    assert_eq!(report["summary"]["analysis_blockers"], 0, "{report}");
    assert_eq!(report["summary"]["specialist_findings"], 2, "{report}");
    let findings = report["tool_diagnostics"].as_array().unwrap();
    for (index, finding) in findings.iter().enumerate() {
        assert_eq!(finding["rule"], "clippy::needless_return");
        assert_eq!(finding["line"], index + 1);
        assert!(finding["column"].as_u64().unwrap() > 0);
        assert!(
            finding["file"]
                .as_str()
                .unwrap()
                .ends_with("member/src/lib.rs")
        );
        assert_eq!(finding["target"]["name"], "check_member");
    }
}

#[test]
fn declared_no_default_feature_check_is_required_and_recorded() {
    let project = Project::new("check-feature-contract");
    project.policy("feature_checks = [\"cargo check --workspace --no-default-features --locked\"]");
    rust_workspace(
        &project.0,
        "#![cfg_attr(not(feature = \"std\"), no_std)]\npub fn text() -> String { String::new() }\n",
    );
    let manifest = project.0.join("member/Cargo.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str("[features]\ndefault=[\"std\"]\nstd=[]\n");
    std::fs::write(manifest, text).unwrap();
    let report = project.check(&["--checks", "typecheck"]);
    assert_eq!(report["accepted"], false);
    let failure = &report["orchestration_violations"][0];
    assert_eq!(failure["step"], "typecheck");
    assert!(
        failure["command"]
            .as_str()
            .unwrap()
            .contains("--no-default-features")
    );
    assert!(
        failure["output"]
            .as_str()
            .unwrap()
            .contains("cannot find type `String`"),
        "{report}"
    );
    assert!(
        report["execution"]["engines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|engine| engine["id"] == "typecheck"
                && engine["required_evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|command| command.as_str().unwrap().contains("--no-default-features")))
    );
}

#[test]
fn clippy_preserves_examples_benchmarks_integration_tests_and_declared_fuzz_targets() {
    let project = Project::new("check-target-scope");
    project.policy(
        "lint = \"cargo clippy --workspace --all-targets --all-features --locked --offline\"",
    );
    rust_workspace(&project.0, "pub fn answer() -> usize { 42 }\n");
    let manifest = project.0.join("member/Cargo.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str("[features]\nfuzzing=[]\n[[example]]\nname='active-example'\npath='cases/example.rs'\n[[bench]]\nname='active-bench'\npath='cases/bench.rs'\nharness=false\n[[test]]\nname='active-integration'\npath='cases/integration.rs'\n[[bin]]\nname='declared-fuzz'\npath='cases/fuzz.rs'\nrequired-features=['fuzzing']\n");
    std::fs::write(manifest, text).unwrap();
    std::fs::create_dir(project.0.join("member/cases")).unwrap();
    for name in ["example", "bench", "integration", "fuzz"] {
        std::fs::write(
            project.0.join(format!("member/cases/{name}.rs")),
            "pub fn exercised() -> usize { return 42; }\nfn main() { let _ = exercised(); }\n",
        )
        .unwrap();
    }
    let report = project.check(&["--checks", "lint"]);
    assert_eq!(report["summary"]["analysis_blockers"], 0, "{report}");
    let findings = report["tool_diagnostics"].as_array().unwrap();
    for target in [
        "active-example",
        "active-bench",
        "active-integration",
        "declared-fuzz",
    ] {
        assert!(
            findings
                .iter()
                .any(|finding| finding["rule"] == "clippy::needless_return"
                    && (finding["target"]["name"] == target
                        || finding["targets"].as_array().is_some_and(|targets| targets
                            .iter()
                            .any(|value| value["name"] == target)))),
            "missing target {target}: {report}"
        );
    }
}

#[test]
fn mutation_requires_passing_original_workspace_before_specialist_execution() {
    use std::os::unix::fs::PermissionsExt;
    let project = Project::new("mutation-workspace-baseline");
    rust_workspace(&project.0, "pub fn value() -> u32 { 1 }\n");
    std::fs::write(
        project.0.join("Cargo.toml"),
        "[workspace]\nmembers=[\"member\",\"broken\"]\nresolver=\"3\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(project.0.join("broken/src")).unwrap();
    std::fs::write(
        project.0.join("broken/Cargo.toml"),
        "[package]\nname=\"broken-baseline\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        project.0.join("broken/src/lib.rs"),
        "#[test]\nfn existing_failure() { assert_eq!(1, 2); }\n",
    )
    .unwrap();
    let lock = fixture_command("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&project.0)
        .output()
        .unwrap();
    assert!(lock.status.success(), "{lock:?}");
    project.policy("");
    let bin = project.0.join("tools");
    std::fs::create_dir(&bin).unwrap();
    let runner = bin.join("cargo-mutants");
    std::fs::write(&runner, "#!/bin/sh\nif [ \"$2\" = --version ]; then echo cargo-mutants 27.1.0; exit 0; fi\nprintf invoked > specialist-invoked\nexit 99\n").unwrap();
    std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let output = fixture_command(env!("CARGO_BIN_EXE_hardgate"))
        .args([
            "evidence",
            "cargo-mutants",
            "--",
            "--package",
            "check-member",
            "--file",
            "member/src/lib.rs",
            "--offline",
        ])
        .env("PATH", path)
        .current_dir(&project.0)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("existing_failure"),
        "{output:?}"
    );
    assert!(!project.0.join("specialist-invoked").exists());
    assert!(
        !project
            .0
            .join(".hardgate/evidence/mutation.json.hardgate.json")
            .exists()
    );
}
