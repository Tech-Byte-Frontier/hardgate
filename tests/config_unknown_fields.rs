use hardgate::config::HardgateConfig;
use std::path::PathBuf;

fn write_config(tag: &str, body: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "hardgate-config-unknown-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hardgate.toml");
    std::fs::write(&path, body).unwrap();
    (dir, path)
}

fn assert_unknown_field(tag: &str, body: &str, unknown: &str, expected: &str) {
    let (dir, path) = write_config(tag, body);
    let error = HardgateConfig::load_or_default(Some(&path)).unwrap_err();
    let rendered = format!("{error:#}");
    assert!(
        rendered.contains("unknown field") && rendered.contains(unknown),
        "error should identify `{unknown}` as unknown: {rendered}"
    );
    assert!(
        rendered.contains(expected),
        "error should list `{expected}` as a valid field: {rendered}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn fixed_config_tables_reject_unknown_fields_with_expected_names() {
    let cases = [
        (
            "root",
            "[gate]\npreset = \"custom\"\nunknown_root = true\n",
            "unknown_root",
            "gate",
        ),
        (
            "gate",
            "[gate]\npreset = \"custom\"\nstrcit = true\n",
            "strcit",
            "strict",
        ),
        (
            "budgets",
            "[gate]\npreset = \"custom\"\n\n[budgets]\nunknown_budgets = true\n",
            "unknown_budgets",
            "files",
        ),
        (
            "file-budgets",
            "[gate]\npreset = \"custom\"\n\n[budgets.files]\nmax_btyes = 12\n",
            "max_btyes",
            "max_bytes",
        ),
        (
            "file-exclusions",
            "[gate]\npreset = \"custom\"\n\n[budgets.files.exclusions]\npathz = []\n",
            "pathz",
            "paths",
        ),
        (
            "function-budgets",
            "[gate]\npreset = \"custom\"\n\n[budgets.functions]\nmax_cyclomtic = 2\n",
            "max_cyclomtic",
            "max_cyclomatic",
        ),
        (
            "anti-gaming",
            "[gate]\npreset = \"custom\"\n\n[anti_gaming]\ndisallow_suppresions = false\n",
            "disallow_suppresions",
            "disallow_suppressions",
        ),
        (
            "invariants",
            "[gate]\npreset = \"custom\"\n\n[invariants]\nruls = []\n",
            "ruls",
            "rules",
        ),
        (
            "invariant-rule",
            "[gate]\npreset = \"custom\"\n\n[[invariants.rules]]\nfrom = \"src/**\"\nmesage = \"boundary\"\n",
            "mesage",
            "message",
        ),
        (
            "clones",
            "[gate]\npreset = \"custom\"\n\n[clones]\nenabeld = false\n",
            "enabeld",
            "enabled",
        ),
        (
            "coverage",
            "[gate]\npreset = \"custom\"\n\n[coverage]\nreprot = \"coverage.info\"\n",
            "reprot",
            "report",
        ),
        (
            "mutation",
            "[gate]\npreset = \"custom\"\n\n[mutation]\nenabeld = false\n",
            "enabeld",
            "enabled",
        ),
        (
            "orchestration",
            "[gate]\npreset = \"custom\"\n\n[orchestration]\ntimout_secs = 1\n",
            "timout_secs",
            "timeout_secs",
        ),
        (
            "analysis",
            "[gate]\npreset = \"custom\"\n\n[analysis]\ndeadcode = true\n",
            "deadcode",
            "dead_code",
        ),
        (
            "dead-code",
            "[gate]\npreset = \"custom\"\n\n[analysis.dead_code]\nentry_pionts = [\"src/main.rs\"]\n",
            "entry_pionts",
            "entry_points",
        ),
        (
            "role",
            "[gate]\npreset = \"custom\"\n\n[roles.source]\nmax_lins = 77\n",
            "max_lins",
            "max_lines",
        ),
        (
            "role-section",
            "[gate]\npreset = \"custom\"\n\n[roles.sourc]\nmax_lines = 77\n",
            "sourc",
            "source",
        ),
        (
            "classification",
            "[gate]\npreset = \"custom\"\n\n[[classification.rules]]\ngloob = \"src/**\"\nrole = \"source\"\n",
            "gloob",
            "glob",
        ),
        (
            "generated",
            "[gate]\npreset = \"custom\"\n\n[generated]\nfreshnes_command = \"pnpm generate\"\n",
            "freshnes_command",
            "freshness_command",
        ),
        (
            "legacy",
            "[gate]\npreset = \"custom\"\n\n[legacy]\nratche = false\n",
            "ratche",
            "ratchet",
        ),
    ];

    for (tag, body, unknown, expected) in cases {
        assert_unknown_field(tag, body, unknown, expected);
    }
}

#[test]
fn file_line_budget_extensions_remain_dynamic_map_keys() {
    let (dir, path) = write_config(
        "dynamic-lines",
        r#"[gate]
preset = "custom"

[budgets.files.max_lines]
rs = 120
custom_extension = 37
"#,
    );
    let config = HardgateConfig::load_or_default(Some(&path)).unwrap();
    assert_eq!(config.budgets.files.max_lines.get("rs"), Some(&120));
    assert_eq!(
        config.budgets.files.max_lines.get("custom_extension"),
        Some(&37)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn role_policies_alias_and_explicit_empty_or_false_values_survive_merge() {
    let (dir, path) = write_config(
        "alias-presence",
        r#"[gate]
preset = "strict-agent"

[role_policies.source]
max_lines = 77
clone_enabled = false

[coverage]
enabled = false
critical_paths = []

[mutation]
enabled = false
reports = []
test_cmd = ""

[analysis.dead_code]
enabled = false
entry_points = []
exclude = []
"#,
    );
    let config = HardgateConfig::load_or_default(Some(&path)).unwrap();

    assert_eq!(config.roles.source.max_lines, Some(77));
    assert_eq!(config.roles.source.clone_enabled, Some(false));
    assert_eq!(config.roles.source.clone_min_lines, Some(5));
    assert!(!config.coverage.enabled);
    assert_eq!(config.coverage.critical_paths, Some(Vec::new()));
    assert!(!config.mutation.enabled);
    assert_eq!(config.mutation.reports, Some(Vec::new()));
    assert_eq!(config.mutation.test_cmd.as_deref(), Some(""));
    assert!(!config.analysis.dead_code.enabled);
    assert!(config.analysis.dead_code.entry_points.is_empty());
    assert!(config.analysis.dead_code.exclude.is_empty());

    let _ = std::fs::remove_dir_all(dir);
}
