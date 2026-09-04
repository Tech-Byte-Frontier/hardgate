use hardgate::config::{ClassificationConfig, ClassificationRule};
use hardgate::discovery::{ClassifiedFile, FileRole, ast_supported, is_inventory_file};
use std::path::Path;

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

#[test]
fn rust_test_module_suffixes_are_test_role_only_for_rust() {
    for path in ["tests.rs", "src/mutate_tests.rs", "src/runner-tests.rs"] {
        assert_eq!(classify(path).role, FileRole::Test, "{path}");
    }
    for path in [
        "src/tests.ts",
        "src/mutate_tests.ts",
        "src/runner-tests.py",
        "src/latest.rs",
    ] {
        assert_eq!(classify(path).role, FileRole::Source, "{path}");
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
