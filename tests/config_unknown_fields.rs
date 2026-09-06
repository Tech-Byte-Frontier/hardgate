#[path = "support/fs.rs"]
mod fs;

use hardgate::config::HardgateConfig;
use std::path::PathBuf;

fn write_config(tag: &str, body: &str) -> (PathBuf, PathBuf) {
    let dir = fs::tempdir(&format!("config-unknown-{tag}"));
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

const CUSTOM_CONFIG_HEADER: &str = "[gate]\npreset = \"custom\"\n\n";

const UNKNOWN_FIELD_CASES: &[&str] = &[
    "root|unknown_root = true\n\n[gate]\npreset = \"custom\"\n|unknown_root|gate",
    "gate|[gate]\npreset = \"custom\"\nstrcit = true\n|strcit|strict",
    "budgets|[budgets]\nunknown_budgets = true\n|unknown_budgets|files",
    "file-budgets|[budgets.files]\nmax_btyes = 12\n|max_btyes|max_bytes",
    "file-exclusions|[budgets.files.exclusions]\npathz = []\n|pathz|paths",
    "function-budgets|[budgets.functions]\nmax_cyclomtic = 2\n|max_cyclomtic|max_cyclomatic",
    "anti-gaming|[anti_gaming]\ndisallow_suppresions = false\n|disallow_suppresions|disallow_suppressions",
    "invariants|[invariants]\nruls = []\n|ruls|rules",
    "invariant-rule|[[invariants.rules]]\nfrom = \"src/**\"\nmesage = \"boundary\"\n|mesage|message",
    "clones|[clones]\nenabeld = false\n|enabeld|enabled",
    "coverage|[coverage]\nreprot = \"coverage.info\"\n|reprot|report",
    "mutation|[mutation]\nenabeld = false\n|enabeld|enabled",
    "orchestration|[orchestration]\ntimout_secs = 1\n|timout_secs|timeout_secs",
    "role|[roles.source]\nmax_lins = 77\n|max_lins|max_lines",
    "role-section|[roles.sourc]\nmax_lines = 77\n|sourc|source",
    "classification-table|[classification]\nrulz = []\n|rulz|rules",
    "classification-rule|[[classification.rules]]\ngloob = \"src/**\"\nrole = \"source\"\n|gloob|glob",
    "generated|[generated]\nfreshnes_command = \"pnpm generate\"\n|freshnes_command|freshness_command",
    "legacy|[legacy]\nratche = false\n|ratche|ratchet",
];

#[test]
fn fixed_config_tables_reject_unknown_fields_with_expected_names() {
    for encoded in UNKNOWN_FIELD_CASES {
        let mut fields = encoded.split('|');
        let tag = fields.next().unwrap();
        let fragment = fields.next().unwrap();
        let unknown = fields.next().unwrap();
        let expected = fields.next().unwrap();
        assert!(
            fields.next().is_none(),
            "invalid unknown-field case: {encoded}"
        );
        let body = if matches!(tag, "root" | "gate") {
            fragment.to_string()
        } else {
            format!("{CUSTOM_CONFIG_HEADER}{fragment}")
        };
        assert_unknown_field(tag, &body, unknown, expected);
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

    let _ = std::fs::remove_dir_all(dir);
}
