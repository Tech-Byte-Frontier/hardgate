use hardgate::config::{
    ClassificationConfig, ClassificationRule, HardgateConfig, LegacyConfig, Preset,
    RolePoliciesConfig, Severity,
};
use hardgate::discovery::{ClassifiedFile, FileRole, ast_supported, is_inventory_file};
use std::path::Path;

#[path = "support/fs.rs"]
mod fs;

fn classify(path: &str) -> ClassifiedFile {
    ClassifiedFile::new(Path::new(path))
}

#[test]
fn builtins_cover_documented_repository_conventions() {
    assert_source_conventions();
    assert_config_conventions();
    assert_test_conventions();
    assert_fixture_conventions();
    assert_generated_conventions();
    assert_migration_conventions();
    assert_vendor_conventions();
}

fn assert_source_conventions() {
    let source_cases = [
        "src/app.js",
        "src/view.jsx",
        "src/service.ts",
        "src/component.tsx",
        "src/worker.mjs",
        "src/worker.cjs",
        "src/worker.mts",
        "src/worker.cts",
        "supabase/functions/mail/index.ts",
        "schema.sql",
        "schema.graphql",
        "schema.gql",
    ];
    for path in source_cases {
        assert_eq!(classify(path).role, FileRole::Source, "{path}");
        assert!(is_inventory_file(Path::new(path)), "{path}");
    }
}

fn assert_config_conventions() {
    for path in [
        "docs/guide.mdx",
        "package.json",
        "package.jsonc",
        "config.toml",
        "config.yaml",
        "config.yml",
    ] {
        let expected = if path.ends_with(".mdx") {
            FileRole::Documentation
        } else {
            FileRole::Config
        };
        assert_eq!(classify(path).role, expected, "{path}");
        assert!(!ast_supported(Path::new(path)), "{path}");
    }
}

fn assert_test_conventions() {
    for path in [
        "src/button.stories.tsx",
        "src/button.test.tsx",
        "src/client.mock.ts",
        "src/__mocks__/client.ts",
        "src/stories/button.tsx",
    ] {
        assert_eq!(classify(path).role, FileRole::Test, "{path}");
    }
}

fn assert_fixture_conventions() {
    for path in [
        "tests/__fixtures__/state.snap",
        "src/__snapshots__/button.snap",
        "src/components/button.fixture.tsx",
        "src/components/fixtures/reducer.tsx",
    ] {
        assert_eq!(classify(path).role, FileRole::Fixture, "{path}");
    }
}

fn assert_generated_conventions() {
    for path in [
        "supabase/database.types.ts",
        "supabase/schema.gen.ts",
        "src/generated/client.ts",
        "src/__generated__/api.ts",
    ] {
        assert_eq!(classify(path).role, FileRole::Generated, "{path}");
    }
}

fn assert_migration_conventions() {
    for path in [
        "supabase/migrations/001_init.sql",
        "migrations/002_add.sql",
        "supabase/seed.sql",
        "seed.sql",
        "data.seed.sql",
    ] {
        assert_eq!(classify(path).role, FileRole::Migration, "{path}");
    }
}

fn assert_vendor_conventions() {
    for path in [
        "node_modules/pkg/index.ts",
        "target/generated.rs",
        "dist/app.js",
        "build/app.js",
        "vendor/lib.rs",
        "venv/lib.py",
        "__pycache__/module.py",
    ] {
        assert_eq!(classify(path).role, FileRole::Vendor, "{path}");
    }
}

#[test]
fn custom_rules_are_ordered_and_cannot_override_vendor_pruning() {
    let config = ClassificationConfig {
        rules: vec![
            ClassificationRule {
                glob: "src/**".to_string(),
                role: FileRole::Fixture,
            },
            ClassificationRule {
                glob: "src/generated/**".to_string(),
                role: FileRole::Source,
            },
        ],
    };
    let first =
        ClassifiedFile::new_with_config(Path::new("/tmp/project/src/generated/api.ts"), &config)
            .unwrap();
    assert_eq!(first.role, FileRole::Fixture);
    assert!(first.reason.contains("custom classification rule 0"));

    let vendor =
        ClassifiedFile::new_with_config(Path::new("/tmp/project/node_modules/src/api.ts"), &config)
            .unwrap();
    assert_eq!(vendor.role, FileRole::Vendor);
}

#[test]
fn classification_rules_reject_empty_invalid_and_duplicate_globs() {
    for glob in ["", "[invalid"] {
        let config = ClassificationConfig {
            rules: vec![ClassificationRule {
                glob: glob.to_string(),
                role: FileRole::Source,
            }],
        };
        assert!(config.validate().is_err(), "{glob:?}");
    }

    let duplicate = ClassificationConfig {
        rules: vec![
            ClassificationRule {
                glob: "src/**".to_string(),
                role: FileRole::Source,
            },
            ClassificationRule {
                glob: "SRC/**".to_string(),
                role: FileRole::Test,
            },
        ],
    };
    assert!(duplicate.validate().is_err());
}

#[test]
fn role_policies_keep_engine_thresholds_independent() {
    let roles = RolePoliciesConfig::for_preset(true);
    assert_eq!(roles.source.severity, Some(Severity::Error));
    assert_eq!(roles.test.severity, Some(Severity::Error));
    assert_eq!(roles.generated.severity, Some(Severity::Ignore));
    assert_eq!(roles.fixture.severity, Some(Severity::Warning));
    assert_eq!(roles.migration.severity, Some(Severity::Error));
    assert_eq!(roles.source.mutation_target, Some(true));
    assert_eq!(roles.test.mutation_target, Some(false));
    assert_eq!(roles.source.clone_enabled, None);
    assert_eq!(roles.test.clone_enabled, None);
    assert_eq!(roles.fixture.clone_enabled, None);
    assert_eq!(roles.generated.clone_enabled, Some(false));
    assert_eq!(roles.migration.clone_enabled, Some(false));
    assert_ne!(roles.source.clone_min_lines, roles.test.clone_min_lines);
    assert_ne!(roles.test.clone_min_lines, roles.fixture.clone_min_lines);
    assert!(roles.validate().is_ok());

    let mut invalid = roles.clone();
    invalid.fixture.mutation_target = Some(true);
    assert!(invalid.validate().is_err());
}

#[test]
fn role_policy_toml_overrides_inherit_preset_values() {
    let root = fs::tempdir("role-merge");
    let path = root.join("hardgate.toml");
    std::fs::write(
        &path,
        r#"
[gate]
preset = "balanced"

[roles.test]
severity = "warning"
max_lines = 300
clone_min_tokens = 123
mutation_target = false

[generated]
enabled = true
freshness_command = "pnpm generate:check"
timeout_secs = 12

[legacy]
reference_branch = "main"
ratchet = false

[orchestration]
timeout_secs = 9
"#,
    )
    .unwrap();
    let config = HardgateConfig::load_or_default(Some(&path)).unwrap();
    assert_eq!(config.roles.test.severity, Some(Severity::Warning));
    assert_eq!(config.roles.test.max_lines, Some(300));
    assert_eq!(config.roles.test.clone_min_lines, Some(12));
    assert_eq!(config.roles.test.clone_min_tokens, Some(123));
    assert_eq!(
        config.generated.freshness_command.as_deref(),
        Some("pnpm generate:check")
    );
    assert_eq!(config.generated.timeout_secs, Some(12));
    assert_eq!(config.legacy.reference_branch.as_deref(), Some("main"));
    assert!(!config.legacy.ratchet);
    assert_eq!(config.orchestration.timeout_secs, Some(9));
}

#[test]
fn clone_enablement_overrides_global_policy_only_when_explicit() {
    let root = fs::tempdir("role-clone-enable-merge");

    let inherited_path = root.join("inherited.toml");
    std::fs::write(
        &inherited_path,
        "[gate]\npreset = \"custom\"\n\n[clones]\nenabled = false\n",
    )
    .unwrap();
    let inherited = HardgateConfig::load_or_default(Some(&inherited_path)).unwrap();
    assert!(!inherited.clones.enabled);
    assert_eq!(inherited.roles.source.clone_enabled, None);

    let override_path = root.join("override.toml");
    std::fs::write(
        &override_path,
        "[gate]\npreset = \"custom\"\n\n[clones]\nenabled = false\n\n[roles.source]\nclone_enabled = true\n",
    )
    .unwrap();
    let overridden = HardgateConfig::load_or_default(Some(&override_path)).unwrap();
    assert!(!overridden.clones.enabled);
    assert_eq!(overridden.roles.source.clone_enabled, Some(true));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn invalid_role_config_fails_during_load() {
    let cases = [
        ("[roles.source]\nmax_lines = 0", "max_lines"),
        ("[roles.test]\nmutation_target = true", "mutation_target"),
        ("[generated]\nenabled = true", "freshness_command"),
        ("[orchestration]\ntimeout_secs = 0", "timeout_secs"),
        (
            "[classification]\n[[classification.rules]]\nglob = \"[bad\"\nrole = \"source\"",
            "glob",
        ),
        (
            "[invariants]\n[[invariants.rules]]\nfrom = \"src/**\"\ndisallow_imports = [\"\"]",
            "disallow_imports",
        ),
        (
            "[invariants]\n[[invariants.rules]]\nfrom = \"src/**\"\ndisallow_imports = [\"[bad\"]",
            "disallow_imports",
        ),
        (
            "[analysis.dead_code]\nentry_points = [\"\"]",
            "entry_points",
        ),
        (
            "[analysis.dead_code]\nentry_points = [\"[bad\"]",
            "entry_points",
        ),
    ];
    for (content, message) in cases {
        let root = fs::tempdir("role-invalid");
        let path = root.join("hardgate.toml");
        std::fs::write(&path, content).unwrap();
        let error = HardgateConfig::load_or_default(Some(&path)).unwrap_err();
        assert!(error.to_string().contains(message), "{error:#}");
    }
}

#[test]
fn enabled_generated_freshness_requires_a_command_in_legacy_mode() {
    let mut config = Preset::LegacyMigration.to_default_config();
    config.generated.enabled = true;
    config.generated.freshness_command = None;

    let error = config.validate().unwrap_err();
    assert!(error.to_string().contains("freshness_command"), "{error:#}");
}

#[test]
fn partial_orchestration_override_preserves_preset_commands() {
    let root = fs::tempdir("role-partial-orchestration");
    let path = root.join("hardgate.toml");
    std::fs::write(
        &path,
        "[gate]\npreset = \"strict-agent\"\n\n[orchestration]\ntimeout_secs = 9\n",
    )
    .unwrap();

    let config = HardgateConfig::load_or_default(Some(&path)).unwrap();
    let expected = Preset::StrictAgent.to_default_config().orchestration;
    assert_eq!(config.orchestration.format_check, expected.format_check);
    assert_eq!(config.orchestration.format, expected.format);
    assert_eq!(config.orchestration.lint, expected.lint);
    assert_eq!(config.orchestration.test_cmd, expected.test_cmd);
    assert_eq!(config.orchestration.timeout_secs, Some(9));
}

#[test]
fn partial_strict_verification_sections_preserve_omitted_preset_defaults() {
    let root = fs::tempdir("partial-strict-verification");
    let path = root.join("hardgate.toml");
    std::fs::write(
        &path,
        r#"
[gate]
preset = "strict-agent"

[coverage]
report = "reports/custom-lcov.info"

[mutation]
min_score = 72.0

[analysis.dead_code]
enabled = true
"#,
    )
    .unwrap();

    let config = HardgateConfig::load_or_default(Some(&path)).unwrap();
    let expected = Preset::StrictAgent.to_default_config();

    assert!(config.coverage.enabled);
    assert_eq!(
        config.coverage.report.as_deref(),
        Some("reports/custom-lcov.info")
    );
    assert_eq!(
        config.coverage.min_line_percent,
        expected.coverage.min_line_percent
    );
    assert_eq!(
        config.coverage.min_function_percent,
        expected.coverage.min_function_percent
    );
    assert_eq!(
        config.coverage.min_branch_percent,
        expected.coverage.min_branch_percent
    );
    assert_eq!(
        config.coverage.max_crap_score,
        expected.coverage.max_crap_score
    );
    assert_eq!(
        config.coverage.critical_paths,
        expected.coverage.critical_paths
    );

    assert!(config.mutation.enabled);
    assert_eq!(config.mutation.min_score, Some(72.0));
    assert_eq!(config.mutation.reports, expected.mutation.reports);
    assert_eq!(config.mutation.test_cmd, expected.mutation.test_cmd);
    assert_eq!(config.mutation.timeout_secs, expected.mutation.timeout_secs);
    assert_eq!(config.mutation.max_mutants, expected.mutation.max_mutants);

    assert!(config.analysis.dead_code.enabled);
    assert_eq!(
        config.analysis.dead_code.entry_points,
        expected.analysis.dead_code.entry_points
    );
    assert_eq!(
        config.analysis.dead_code.exclude,
        expected.analysis.dead_code.exclude
    );
}

#[test]
fn explicit_false_and_empty_verification_values_override_strict_defaults() {
    let root = fs::tempdir("explicit-strict-verification");
    let path = root.join("hardgate.toml");
    std::fs::write(
        &path,
        r#"
[gate]
preset = "strict-agent"

[coverage]
enabled = false
report = ""
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
    )
    .unwrap();

    let config = HardgateConfig::load_or_default(Some(&path)).unwrap();
    let expected = Preset::StrictAgent.to_default_config();

    assert!(!config.coverage.enabled);
    assert_eq!(config.coverage.report.as_deref(), Some(""));
    assert_eq!(config.coverage.critical_paths, Some(Vec::new()));
    assert_eq!(
        config.coverage.min_line_percent,
        expected.coverage.min_line_percent
    );
    assert_eq!(
        config.coverage.max_crap_score,
        expected.coverage.max_crap_score
    );

    assert!(!config.mutation.enabled);
    assert_eq!(config.mutation.reports, Some(Vec::new()));
    assert_eq!(config.mutation.test_cmd.as_deref(), Some(""));
    assert_eq!(config.mutation.min_score, expected.mutation.min_score);
    assert_eq!(config.mutation.timeout_secs, expected.mutation.timeout_secs);
    assert_eq!(config.mutation.max_mutants, expected.mutation.max_mutants);

    assert!(!config.analysis.dead_code.enabled);
    assert!(config.analysis.dead_code.entry_points.is_empty());
    assert!(config.analysis.dead_code.exclude.is_empty());
}

#[test]
fn generated_presets_and_runtime_defaults_have_matching_semantics() {
    let strict =
        HardgateConfig::load_or_default(Some(Path::new("/definitely/missing/hardgate.toml")))
            .unwrap();
    let strict_preset = Preset::StrictAgent.to_default_config();
    assert_eq!(strict.gate.strict, strict_preset.gate.strict);
    assert_eq!(
        strict.gate.enforce_classified_sources,
        strict_preset.gate.enforce_classified_sources
    );
    assert_eq!(strict.coverage.enabled, strict_preset.coverage.enabled);
    assert_eq!(strict.mutation.enabled, strict_preset.mutation.enabled);
    for preset in [
        Preset::StrictAgent,
        Preset::Balanced,
        Preset::LegacyMigration,
        Preset::Custom,
    ] {
        let expected = preset.to_default_config();
        let parsed: HardgateConfig =
            toml::from_str(&HardgateConfig::generate_toml_template(preset)).unwrap();
        assert_eq!(parsed.gate.strict, expected.gate.strict, "{preset:?}");
        assert_eq!(
            parsed.gate.enforce_classified_sources,
            expected.gate.enforce_classified_sources
        );
        assert_eq!(parsed.coverage.enabled, expected.coverage.enabled);
        assert_eq!(parsed.mutation.enabled, expected.mutation.enabled);
        assert_eq!(parsed.roles, expected.roles);
        assert_eq!(parsed.generated, expected.generated);
        assert_eq!(parsed.legacy, expected.legacy);
        assert_eq!(
            parsed.orchestration.timeout_secs,
            expected.orchestration.timeout_secs
        );
    }

    let legacy = LegacyConfig::for_preset(true);
    assert_eq!(legacy.reference_branch.as_deref(), Some("origin/main"));
    assert!(legacy.ratchet);
    assert!(LegacyConfig::for_preset(false).reference_branch.is_none());
}

#[test]
fn removed_timeout_knob_is_rejected_instead_of_ignored() {
    assert!(!HardgateConfig::generate_toml_template(Preset::Custom).contains("reject_timeouts"));
    let root = fs::tempdir("custom-mutation-default");
    let path = root.join("hardgate.toml");
    std::fs::write(
        &path,
        "[gate]\npreset = \"custom\"\n\n[mutation]\nenabled = true\nreject_timeouts = false\n",
    )
    .unwrap();
    let error = HardgateConfig::load_or_default(Some(&path)).unwrap_err();
    assert!(
        format!("{error:#}").contains("reject_timeouts"),
        "{error:#}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
