use super::*;
use crate::evidence::Producer;

#[test]
fn partition_inventory_preserves_execution_and_test_boundaries() {
    let root = crate::fs_tests::tempdir("partition-roles");
    for (name, body) in [
        ("source.rs", "pub fn source() -> u8 { 1 }"),
        (
            "helper.rs",
            "#[cfg(test)] mod checks { fn test_helper() {} }",
        ),
        ("constants.rs", "pub const ANSWER: u8 = 42;"),
        ("styles.css", "body { color: red; }"),
        ("types.ts", "export interface Shape { count: number; }"),
        ("runtime.js", "export function runtime() { return 1; }"),
    ] {
        std::fs::write(root.join(name), body).unwrap();
    }
    assert_eq!(
        source_inventory(&root, &HardgateConfig::default()).unwrap(),
        vec![PathBuf::from("runtime.js"), PathBuf::from("source.rs")]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn named_rust_partition_owns_command_overrides_and_artifact_format() {
    let root = crate::fs_tests::tempdir("partition-owned-options");
    std::fs::write(root.join("source.rs"), "pub fn source() -> u8 { 1 }").unwrap();
    let mut context = ConfigContext::load_from(&root, None).unwrap();
    context.config.evidence.producers.insert(
        "native".into(),
        toml::from_str("producer='cargo-mutants'\nsources=['source.rs']\ntimeout_secs=20").unwrap(),
    );
    let base = EvidenceOptions {
        producer: Producer::CargoMutants,
        producer_config: Some("native".into()),
        name: None,
        toolchain: None,
        timeout_secs: 30,
        args: vec![],
    };
    let mut selected = base.clone();
    let partition = resolve(&mut selected, &context).unwrap().unwrap();
    assert_eq!(partition.sources, vec![PathBuf::from("source.rs")]);
    assert_eq!(selected.timeout_secs, 20);
    assert_eq!(
        reports(&context.config, EvidenceKind::Mutation),
        vec![".hardgate/evidence/native.json"]
    );
    for option in [
        EvidenceOptions {
            args: vec!["--all-features".into()],
            ..base.clone()
        },
        EvidenceOptions {
            toolchain: Some("stable".into()),
            ..base.clone()
        },
        EvidenceOptions {
            name: Some("other".into()),
            ..base
        },
    ] {
        assert!(resolve(&mut option.clone(), &context).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}
