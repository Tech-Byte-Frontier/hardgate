use hardgate::config::{ClassificationConfig, ClassificationRule};
use hardgate::discovery::{ClassifiedFile, FileRole};
use std::path::Path;

fn role(path: &str) -> FileRole {
    ClassifiedFile::new(Path::new(path)).role
}

fn assert_source_paths(encoded_paths: &str) {
    for path in encoded_paths.lines().filter(|path| !path.is_empty()) {
        assert_eq!(role(path), FileRole::Source, "{path}");
    }
}

const LOOKALIKE_DIRECTORY_PATHS: &str = include_str!("common/classification_directory_paths.txt");
const LOOKALIKE_FILE_NAMES: &str = include_str!("common/classification_file_names.txt");

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
    assert_source_paths(LOOKALIKE_DIRECTORY_PATHS);
}

#[test]
fn lookalike_file_names_remain_source() {
    assert_source_paths(LOOKALIKE_FILE_NAMES);
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
