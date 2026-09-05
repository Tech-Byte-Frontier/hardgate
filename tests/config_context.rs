#[path = "common/cli.rs"]
mod cli;

use cli::{Fixture, assert_status, json, run, stdout};
use hardgate::config::ConfigContext;
use std::path::Path;

const POLICY: &str = "[gate]\npreset = 'custom'\nname = 'parent-policy'\nstrict = true\n\n[budgets.functions]\nmax_parameters = 1\n";

fn fixture(tag: &str) -> Fixture {
    let fixture = Fixture::new("config-context", tag, Some(POLICY));
    fixture.write(
        "src/value.ts",
        "export function value(a: number, b: number) { return a + b; }\n",
    );
    fixture
}

#[test]
fn nested_invocations_share_policy_scope_and_diagnostic_paths() {
    let fixture = fixture("nested");
    let root = json(&run(&fixture, &["check", "--json", "src/value.ts"]));
    let nested = json(&run(&fixture.join("src"), &["check", "--json", "value.ts"]));
    assert_eq!(root["passed"], false);
    assert_eq!(
        root["complexity_violations"],
        nested["complexity_violations"]
    );
    assert_eq!(root["files_checked"], nested["files_checked"]);
    let scan = json(&run(&fixture.join("src"), &["scan", "value.ts", "--json"]));
    assert_eq!(root["complexity_violations"], scan["complexity_violations"]);
}

#[test]
fn explicit_policy_overrides_nearest_and_missing_explicit_never_defaults() {
    let fixture = fixture("explicit");
    fixture.write(
        "alternate.toml",
        "[gate]\npreset = 'custom'\nname = 'alternate'\n",
    );
    fixture.write(
        "src/hardgate.toml",
        "[gate]\npreset = 'custom'\nname = 'nested'\n",
    );
    let root = fixture.join("src");
    let local = json(&run(&root, &["config", "--format", "json"]));
    assert_eq!(local["effective"]["gate"]["name"], "nested");
    let explicit = json(&run(
        &root,
        &[
            "config",
            "--config",
            "../alternate.toml",
            "--format",
            "json",
        ],
    ));
    assert_eq!(explicit["effective"]["gate"]["name"], "alternate");
    assert_eq!(explicit["root"], fixture.to_string_lossy().as_ref());
    let missing = run(&root, &["--config", "missing.toml", "check", "--json"]);
    assert_status(&missing, false, "missing explicit config");
    assert!(
        json(&missing)["message"]
            .as_str()
            .unwrap()
            .contains("Explicit config path")
    );
}

#[test]
fn discovery_stops_at_nested_repository_and_worktree_boundaries() {
    let fixture = fixture("boundary");
    for (name, git_file) in [("repository", false), ("worktree", true)] {
        let root = fixture.join(name);
        std::fs::create_dir_all(root.join("src")).unwrap();
        if git_file {
            std::fs::write(root.join(".git"), "gitdir: elsewhere\n").unwrap();
        } else {
            std::fs::create_dir(root.join(".git")).unwrap();
        }
        let context = ConfigContext::load_from(&root.join("src"), None).unwrap();
        assert_eq!(context.root, root);
        assert!(context.config_path.is_none());
        assert_eq!(
            context.config.gate.preset,
            hardgate::config::Preset::StrictAgent
        );
    }
}

#[test]
fn orchestration_runs_at_policy_root_and_inspection_executes_nothing() {
    let fixture = fixture("orchestration");
    fixture.write(
        "hardgate.toml",
        &format!("{POLICY}\n[orchestration]\nformat = \"sh -c 'pwd > location.txt'\"\n"),
    );
    let nested = fixture.join("src");
    let inspect = run(&nested, &["config"]);
    assert_status(&inspect, true, "config inspection");
    assert!(!fixture.join("location.txt").exists());
    let rendered = stdout(&inspect);
    let effective: hardgate::config::HardgateConfig = toml::from_str(&rendered).unwrap();
    assert_eq!(effective.budgets.functions.max_parameters, Some(1));
    assert_status(&run(&nested, &["fmt"]), true, "nested formatter");
    let location = std::fs::read_to_string(fixture.join("location.txt")).unwrap();
    assert_eq!(Path::new(location.trim()), fixture.as_ref());
}

#[test]
fn report_paths_follow_policy_but_cli_overrides_follow_invocation() {
    let fixture = fixture("reports");
    fixture.write("src/value.ts", "export const value = 1;\n");
    fixture.write("hardgate.toml", "[gate]\npreset = 'custom'\n\n[coverage]\nenabled = true\nreport = 'coverage.info'\nmin_line_percent = 100.0\n\n[mutation]\nenabled = true\nreports = ['mutation.json']\nmin_score = 100.0\n");
    fixture.write(
        "coverage.info",
        "SF:src/value.ts\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    );
    fixture.write("mutation.json", "{\"killed\":1}");
    let nested = fixture.join("src");
    assert_status(
        &run(&nested, &["verify", "--json", "value.ts"]),
        true,
        "policy-relative reports",
    );
    fixture.write("src/override.json", "{\"survived\":1}");
    let failed = run(
        &nested,
        &[
            "verify",
            "--json",
            "value.ts",
            "--mutation-report",
            "override.json",
            "--coverage-report",
            "../coverage.info",
        ],
    );
    assert_status(&failed, false, "invocation-relative override");
    assert!(
        !json(&failed)["mutation_violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn a_broken_implicit_policy_link_is_an_error() {
    let fixture = fixture("broken-policy");
    std::fs::remove_file(fixture.join("hardgate.toml")).unwrap();
    std::os::unix::fs::symlink("missing.toml", fixture.join("hardgate.toml")).unwrap();
    assert!(ConfigContext::load_from(&fixture.join("src"), None).is_err());
}

#[test]
fn strict_no_config_matches_generated_template_sections() {
    use hardgate::config::{HardgateConfig, Preset};
    let absent = Fixture::new("config-context", "implicit-default", None);
    let runtime = ConfigContext::load_from(&absent, None).unwrap().config;
    assert!(HardgateConfig::load_or_default(Some(&absent.join("missing.toml"))).is_err());
    let template = HardgateConfig::generate_toml_template(Preset::StrictAgent);
    let generated: HardgateConfig = toml::from_str(&template).unwrap();
    assert_eq!(
        toml::Value::try_from(&runtime).unwrap(),
        toml::Value::try_from(&generated).unwrap()
    );
    absent.write("hardgate.toml", &template);
    let loaded = HardgateConfig::load_or_default(Some(&absent.join("hardgate.toml"))).unwrap();
    assert_eq!(
        toml::Value::try_from(&loaded).unwrap(),
        toml::Value::try_from(&runtime).unwrap()
    );
}
