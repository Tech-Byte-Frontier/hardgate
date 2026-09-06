use hardgate::config::{ClassificationConfig, ClassificationRule};
use hardgate::discovery::classification::{PreparedClassifier, classify_with_config};
use hardgate::discovery::{ClassifiedFile, FileRole};
use std::path::Path;

fn config(rules: &[(&str, FileRole)]) -> ClassificationConfig {
    ClassificationConfig {
        rules: rules
            .iter()
            .map(|(glob, role)| ClassificationRule {
                glob: (*glob).to_string(),
                role: *role,
            })
            .collect(),
    }
}

#[test]
fn prepared_classifier_matches_builtin_and_compatibility_results() {
    let config = ClassificationConfig::default();
    let prepared = PreparedClassifier::new(&config).unwrap();

    for path in [
        "src/value.rs",
        "src/value.mjs",
        "src/value.tsx",
        "tests/value.test.ts",
        "src/generated/value.ts",
        "docs/guide.mdx",
        "schema.sql",
    ] {
        let classified = prepared.classify(Path::new(path));
        assert_eq!(classified, ClassifiedFile::new(Path::new(path)));
        assert_eq!(
            classified,
            ClassifiedFile::new_with_config(Path::new(path), &config).unwrap()
        );
        assert_eq!(
            (classified.role, classified.reason.clone()),
            classify_with_config(Path::new(path), &config).unwrap(),
            "compatibility helper diverged for {path}"
        );
    }
}

#[test]
fn prepared_classifier_preserves_order_case_and_backslash_candidates() {
    let config = config(&[
        ("src/**", FileRole::Fixture),
        ("src/special.ts", FileRole::Source),
    ]);
    let prepared = PreparedClassifier::new(&config).unwrap();

    let classified = prepared.classify(Path::new(r"C:\Repo\SRC\special.ts"));
    assert_eq!(classified.role, FileRole::Fixture);
    assert_eq!(classified.reason, "custom classification rule 0: src/**");
    assert_eq!(classified.path, Path::new(r"C:\Repo\SRC\special.ts"));
    assert!(classified.ast_supported);
}

#[test]
fn prepared_classifier_keeps_vendor_boundary_authoritative() {
    let config = config(&[("**", FileRole::Source)]);
    let prepared = PreparedClassifier::new(&config).unwrap();

    let classified = prepared.classify(Path::new("project/node_modules/pkg/index.mjs"));
    assert_eq!(classified.role, FileRole::Vendor);
    assert_eq!(classified.reason, "dependency or build-output directory");
    assert!(classified.ast_supported);
}

#[test]
fn prepared_classifier_rejects_invalid_globs_before_classification() {
    let config = config(&[("[invalid", FileRole::Source)]);
    let error = PreparedClassifier::new(&config)
        .err()
        .expect("invalid classification globs must fail during preparation");

    assert!(
        error
            .to_string()
            .contains("Invalid classification glob `[invalid`")
    );
}

#[test]
fn prepared_classifier_marks_all_supported_javascript_extensions() {
    let prepared = PreparedClassifier::new(&ClassificationConfig::default()).unwrap();

    for extension in ["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"] {
        let path = format!("src/module.{extension}");
        let classified = prepared.classify(Path::new(&path));
        assert_eq!(classified.role, FileRole::Source, "{path}");
        assert!(classified.ast_supported, "{path}");
    }
}

#[test]
fn lockfiles_are_classified_as_generated() {
    let prepared = PreparedClassifier::new(&ClassificationConfig::default()).unwrap();

    for lockfile in [
        "pnpm-lock.yaml",
        "package-lock.json",
        "yarn.lock",
        "Cargo.lock",
        "bun.lock",
    ] {
        let classified = prepared.classify(Path::new(lockfile));
        assert_eq!(
            classified.role,
            FileRole::Generated,
            "lockfile {lockfile} must be classified as Generated"
        );
        assert_eq!(classified.reason, "lockfile convention");
        assert!(!classified.ast_supported);
    }
}
