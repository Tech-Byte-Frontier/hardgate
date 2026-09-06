use super::*;
use crate::fs_tests;

#[test]
fn cargo_mutants_test_only_paths_and_inline_spans_are_not_production_evidence() {
    let root = fs_tests::tempdir("mutation-rust-ownership");
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn production() -> i32 { 1 }\n#[cfg(test)]\nmod helper;\n#[cfg(test)]\nfn check() -> i32 { 2 }\n").unwrap();
    std::fs::write(root.join("src/helper.rs"), "pub fn helper() -> i32 { 3 }\n").unwrap();
    let inputs = Snapshot::capture(&root).unwrap();
    let config = crate::config::HardgateConfig::default();
    for (file, line, valid) in [
        ("src/lib.rs", 1, true),
        ("src/lib.rs", 5, false),
        ("src/helper.rs", 1, false),
    ] {
        let value = serde_json::json!({"outcomes": [{"scenario": {"Mutant": {"file":file, "span": {"start": {"line":line,"column":1}, "end":{"line":line,"column":2}}}}}]});
        let result = validate(
            &value,
            Producer::CargoMutants,
            &EvidenceInputs {
                snapshot: &inputs,
                config: &config,
                root: &root,
            },
        );
        assert_eq!(result.is_ok(), valid, "{file}:{line}: {result:?}");
        if let Err(error) = result {
            assert!(
                error.to_string().contains("test-only Rust code"),
                "{error:#}"
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mutation_scope_rejects_non_ast_and_disabled_roles_and_handles_crlf_coordinates() {
    let root = fs_tests::tempdir("mutation-role-scope");
    std::fs::write(root.join("data.toml"), "value=1\n").unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn value() {}\r\n").unwrap();
    let inputs = Snapshot::capture(&root).unwrap();
    let mut config = crate::config::HardgateConfig::default();
    let value = serde_json::json!({"files":{"data.toml":{}}});
    let context = EvidenceInputs {
        snapshot: &inputs,
        config: &config,
        root: &root,
    };
    assert!(validate(&value, Producer::Stryker, &context).is_err());
    config.roles.source.mutation_target = Some(false);
    let value = serde_json::json!({"files":{"lib.rs":{}}});
    let context = EvidenceInputs {
        snapshot: &inputs,
        config: &config,
        root: &root,
    };
    assert!(validate(&value, Producer::Stryker, &context).is_err());
    let point = serde_json::json!({"line":2,"column":1});
    assert_eq!(position("pub fn value() {}\r\n", &point).unwrap(), 19);
    std::fs::remove_dir_all(root).unwrap();
}
