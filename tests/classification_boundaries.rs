use hardgate::config::{ClassificationConfig, ClassificationRule};
use hardgate::discovery::{ClassifiedFile, FileRole};
use std::path::Path;

fn role(path: &str) -> FileRole {
    ClassifiedFile::new(Path::new(path)).role
}

#[test]
fn builtins_retain_positive_role_conventions() {
    let cases = [
        ("src/generated/client.ts", FileRole::Generated),
        ("src/__generated__/client.ts", FileRole::Generated),
        ("src/gen/client.ts", FileRole::Generated),
        ("src/client.generated.ts", FileRole::Generated),
        ("src/client.gen.ts", FileRole::Generated),
        ("supabase/database.types.ts", FileRole::Generated),
        ("supabase/schema.gen.ts", FileRole::Generated),
        ("tests/__fixtures__/state.snap", FileRole::Fixture),
        ("src/fixtures/reducer.tsx", FileRole::Fixture),
        ("src/__snapshots__/button.snap", FileRole::Fixture),
        ("src/components/button.fixture.tsx", FileRole::Fixture),
        ("src/components/button.fixtures.tsx", FileRole::Fixture),
        ("src/button.test.tsx", FileRole::Test),
        ("src/button.spec.tsx", FileRole::Test),
        ("src/button.stories.tsx", FileRole::Test),
        ("src/button.mock.ts", FileRole::Test),
        ("src/__tests__/button.ts", FileRole::Test),
        ("src/__mocks__/button.ts", FileRole::Test),
        ("src/stories/button.tsx", FileRole::Test),
        ("supabase/migrations/001_init.sql", FileRole::Migration),
        ("supabase/migration/legacy.sql", FileRole::Migration),
        ("supabase/seed.sql", FileRole::Migration),
        ("supabase/seed.ts", FileRole::Migration),
        ("supabase/data.seed.sql", FileRole::Migration),
        ("supabase/001.migration.sql", FileRole::Migration),
        ("config/runtime.ts", FileRole::Config),
        ("configs/runtime.ts", FileRole::Config),
        ("src/vite.config.ts", FileRole::Config),
        ("src/jest.config.cjs", FileRole::Config),
        ("src/eslint.config.js", FileRole::Config),
        ("src/playwright.config.ts", FileRole::Config),
        (".eslintrc.js", FileRole::Config),
        (".babelrc.js", FileRole::Config),
        ("config/generated/client.ts", FileRole::Generated),
        ("config/tests/button.ts", FileRole::Test),
        ("config/fixtures/state.ts", FileRole::Fixture),
        ("config/migrations/001_init.sql", FileRole::Migration),
    ];

    for (path, expected) in cases {
        assert_eq!(role(path), expected, "{path}");
    }
}

#[test]
fn lookalike_directory_components_remain_source() {
    let paths = [
        "src/not-generated/client.ts",
        "src/not-gen/client.ts",
        "src/regenerated/client.ts",
        "src/not__generated__/client.ts",
        "src/latest/button.tsx",
        "src/contests/button.tsx",
        "src/mockery/client.ts",
        "src/storybooked/button.tsx",
        "src/not-fixtures/state.tsx",
        "src/fixtures-old/state.tsx",
        "src/not-snapshots/state.tsx",
        "src/not__fixtures__/state.tsx",
        "src/not__snapshots__/state.tsx",
        "src/not-tests/button.ts",
        "src/not-test/button.ts",
        "src/not-mocks/button.ts",
        "src/not-stories/button.tsx",
        "src/not__tests__/button.ts",
        "src/not__mocks__/button.ts",
        "src/not-migration/schema.sql",
        "src/not-migrations/schema.sql",
        "src/migrationary/schema.sql",
        "src/not-node_modules/index.ts",
        "src/not-target/index.rs",
        "src/not-dist/index.js",
        "src/not-build/index.ts",
        "src/building/index.ts",
        "src/not-vendor/index.rs",
    ];

    for path in paths {
        assert_eq!(role(path), FileRole::Source, "{path}");
    }
}

#[test]
fn lookalike_file_names_remain_source() {
    let paths = [
        "src/widget.snapshot.ts",
        "src/widget.snapshots.ts",
        "src/widget.testimony.ts",
        "src/widget.testing.ts",
        "src/widget.specimen.ts",
        "src/widget.stories-old.tsx",
        "src/widget.mocking.ts",
        "src/generated.ts",
        "src/gen.ts",
        "src/__generated__.ts",
        "src/fixtures.ts",
        "src/snapshots.ts",
        "src/test.ts",
        "src/tests.ts",
        "src/__tests__.ts",
        "src/mocks.ts",
        "src/__mocks__.ts",
        "src/stories.tsx",
        "src/__fixtures__.ts",
        "src/__snapshots__.ts",
        "src/migration.sql",
        "src/migrations.sql",
        "src/migration.ts",
        "src/migrations.ts",
        "src/config.ts",
        "src/configs.ts",
        "src/seedling.sql",
        "src/reconfigure.ts",
        "src/configured.ts",
        "src/configurator.ts",
        "src/configs-old/runtime.ts",
        "src/not-config/runtime.ts",
        "src/eslintrc.js",
        "src/.eslintrcish.js",
        ".eslintrcish.js",
        "src/node_modules.ts",
        "src/target.rs",
        "src/dist.js",
        "src/build.ts",
        "src/vendor.rs",
    ];

    for path in paths {
        assert_eq!(role(path), FileRole::Source, "{path}");
    }
}

#[test]
fn directory_and_filename_conventions_accept_windows_separators() {
    let cases = [
        (r"src\generated\client.ts", FileRole::Generated),
        (r"tests\__fixtures__\state.snap", FileRole::Fixture),
        (r"src\__tests__\button.ts", FileRole::Test),
        (r"supabase\migrations\001_init.sql", FileRole::Migration),
        (r"configs\runtime.ts", FileRole::Config),
        (r"src\vite.config.ts", FileRole::Config),
        (r"src\not-config\runtime.ts", FileRole::Source),
        (r"src\not-generated\client.ts", FileRole::Source),
        (r"src\latest\button.ts", FileRole::Source),
        (r"repo\node_modules\client.ts", FileRole::Vendor),
    ];

    for (path, expected) in cases {
        assert_eq!(role(path), expected, "{path}");
    }
}

#[test]
fn custom_rules_stay_ordered_while_vendor_is_authoritative() {
    let config = ClassificationConfig {
        rules: vec![
            ClassificationRule {
                glob: "src/**".to_string(),
                role: FileRole::Fixture,
            },
            ClassificationRule {
                glob: "src/special.ts".to_string(),
                role: FileRole::Source,
            },
        ],
    };

    let ordered = ClassifiedFile::new_with_config(Path::new("src/special.ts"), &config).unwrap();
    assert_eq!(ordered.role, FileRole::Fixture);
    assert!(ordered.reason.contains("custom classification rule 0"));

    let vendor =
        ClassifiedFile::new_with_config(Path::new("src/node_modules/special.ts"), &config).unwrap();
    assert_eq!(vendor.role, FileRole::Vendor);
}
