#![cfg(target_os = "linux")]
#[path = "common/cli.rs"]
mod cli;
use cli::{Fixture, json, run, stdout};

fn fixture(tag: &str, command: &str) -> Fixture {
    let f = Fixture::new("check-lifecycle", tag, None);
    f.write("src/index.ts", "export const answer = 42;\n");
    f.write("hardgate.toml", &format!("[gate]\npreset='balanced'\n[orchestration]\nformat_check='true'\nlint='true'\ntest_cmd={}\n", serde_json::to_string(command).unwrap()));
    f
}

#[test]
fn an_inherited_uv_cache_is_redirected_into_the_disposable_workspace() {
    let cache = Fixture::new("check-lifecycle", "host-uv-cache", None);
    cache.write("sentinel", "host cache stays unchanged\n");
    let f = fixture(
        "uv-cache",
        "sh -c 'mkdir -p \"$UV_CACHE_DIR\"; printf cached > \"$UV_CACHE_DIR/entry\"'",
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_hardgate"))
        .current_dir(&f.0)
        .env("UV_CACHE_DIR", &cache.0)
        .args(["check", "--checks", "tests", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(!cache.join("entry").exists());
    assert_eq!(
        std::fs::read_to_string(cache.join("sentinel")).unwrap(),
        "host cache stays unchanged\n"
    );
    assert!(!f.join(".hardgate/evidence/tmp/uv/entry").exists());
}

#[test]
fn ordinary_cache_writes_are_disposable_but_ignored_inputs_remain_protected() {
    let command = "sh -c 'mkdir -p .ruff_cache/0.15.11 .import_linter_cache scripts/nested/__pycache__ .pytest_cache/v/cache; printf cache > .ruff_cache/0.15.11/12345; printf cache > .import_linter_cache/project.meta.json; printf cache > scripts/nested/__pycache__/helper.cpython-311.pyc; printf cache > .pytest_cache/v/cache/nodeids; printf cache > .eslintcache'";
    let f = fixture("cache", command);
    f.write(".gitignore", ".ruff_cache/\n.import_linter_cache/\n__pycache__/\n.pytest_cache/\n.eslintcache\nignored-required/\n");
    f.write("ignored-required/tool.toml", "required=true\n");
    f.write(
        "ignored-required/source.ts",
        "export const required = true;\n",
    );
    let output = run(&f, &["check", "--json"]);
    cli::assert_status(&output, true, "verification cache outputs");
    assert_eq!(json(&output)["accepted"], true);
    assert!(!f.join(".ruff_cache").exists());
    assert!(!f.join("scripts").exists());
    for path in [
        "ignored-required/tool.toml",
        "ignored-required/source.ts",
        "src/index.ts",
        "hardgate.toml",
        ".ruff_cache/required.ts",
    ] {
        if path.ends_with("required.ts") {
            f.write(path, "export const original = 1;\n");
        }
        let original = std::fs::read(f.join(path)).unwrap();
        let config = std::fs::read_to_string(f.join("hardgate.toml")).unwrap();
        let bad = format!("sh -c 'printf changed > {path}'");
        let replacement = config.replace(
            &serde_json::to_string(command).unwrap(),
            &serde_json::to_string(&bad).unwrap(),
        );
        f.write("hardgate.toml", &replacement);
        let protected = if path == "hardgate.toml" {
            replacement.as_bytes()
        } else {
            &original
        };
        let result = run(&f, &["check", "--checks", "tests", "--json"]);
        assert_eq!(result.status.code(), Some(2), "{path}: {}", stdout(&result));
        assert_eq!(std::fs::read(f.join(path)).unwrap(), protected, "{path}");
        f.write("hardgate.toml", &config);
    }
}

#[test]
fn ignored_virtualenv_interpreters_resolve_without_allowing_arbitrary_external_links() {
    let f = fixture(
        "venv",
        ".venv/bin/python3 -c 'import sys; assert sys.prefix != sys.base_prefix'",
    );
    f.write(".gitignore", ".venv/\n");
    let setup = std::process::Command::new("python3")
        .args(["-m", "venv", "--without-pip"])
        .arg(f.join(".venv"))
        .output()
        .unwrap();
    assert!(setup.status.success(), "{setup:?}");
    let output = run(&f, &["check", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert_eq!(json(&output)["accepted"], true);
    // An environment marker is not permission to follow unrelated external data.
    std::os::unix::fs::symlink("/etc/passwd", f.join(".venv/bin/other")).unwrap();
    let output = run(&f, &["check", "--checks", "tests", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).contains("leaves the workspace"));
}

#[test]
fn restricted_tmp_reports_the_actual_runtime_tmpdir_and_works_when_used() {
    let f = fixture("tmp-denied", "mktemp -d /tmp/finance-hardgate.XXXXXXXX");
    let output = run(&f, &["check", "--checks", "tests", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let value = json(&output);
    let failure = &value["orchestration_violations"][0];
    let text = failure["output"].as_str().unwrap();
    assert!(text.contains("Permission denied"), "{text}");
    assert!(text.contains("TMPDIR="), "{text}");
    assert!(
        text.contains("${TMPDIR:-/tmp}/finance-hardgate.XXXXXXXX"),
        "{text}"
    );
    assert!(text.contains("Hardgate containment"), "{text}");
    assert!(
        failure["recommendation"]
            .as_str()
            .unwrap()
            .contains("TMPDIR=")
    );
    let agent = run(
        &f,
        &[
            "check", "--checks", "tests", "--engine", "clones", "--format", "agent",
        ],
    );
    assert!(stdout(&agent).contains("Runtime writable TMPDIR="));
    let good = fixture(
        "tmp-working",
        "sh -c 'created=$(mktemp -d \"${TMPDIR:-/tmp}/finance-hardgate.XXXXXXXX\"); test -d \"$created\"; rmdir \"$created\"'",
    );
    let output = run(&good, &["check", "--checks", "tests", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
}

#[test]
fn declared_coverage_output_is_disposable_and_cannot_establish_evidence() {
    let f = fixture(
        "coverage",
        "sh -c 'mkdir -p coverage; printf generated > coverage/lcov.info'",
    );
    let config = std::fs::read_to_string(f.join("hardgate.toml")).unwrap();
    f.write(
        "hardgate.toml",
        &format!("{config}\n[coverage]\nenabled=true\nreport='coverage/lcov.info'\n"),
    );
    let output = run(&f, &["check", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert!(
        !value["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["step"] == "test"),
        "{value}"
    );
    assert!(
        value["orchestration_violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["step"] == "coverage-report")
    );
    assert!(!f.join("coverage/lcov.info").exists());
    assert!(!f.join("coverage/lcov.info.hardgate.json").exists());
    assert_eq!(
        std::fs::read_to_string(f.join("src/index.ts")).unwrap(),
        "export const answer = 42;\n"
    );
}

#[test]
fn explicit_cache_named_inputs_and_source_named_reports_stay_protected() {
    for (path, extra) in [
        (
            ".eslintcache",
            "[[classification.rules]]\nglob='.eslintcache'\nrole='config'\n",
        ),
        ("src/index.ts", "[coverage]\nreport='src/index.ts'\n"),
        (
            "coverage/required.info",
            "[coverage]\nreport='coverage/required.info'\n[[classification.rules]]\nglob='coverage/required.info'\nrole='source'\n",
        ),
    ] {
        let f = fixture("required", &format!("sh -c 'printf changed > {path}'"));
        f.write(path, "original input\n");
        let config = std::fs::read_to_string(f.join("hardgate.toml")).unwrap();
        f.write("hardgate.toml", &format!("{config}\n{extra}"));
        let output = run(&f, &["check", "--checks", "tests", "--json"]);
        assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
        assert_eq!(
            std::fs::read_to_string(f.join(path)).unwrap(),
            "original input\n"
        );
    }
}
