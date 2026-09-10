use super::*;
use crate::config::{MutationScope, ProducerConfig};
use crate::evidence::{EvidenceOptions, Producer, partitions::Partition};

#[test]
fn exhaustive_rust_scope_rejects_filters_and_unreviewed_native_settings() {
    let root = crate::fs_tests::tempdir("exhaustive-rust-scope");
    let config: ProducerConfig =
        toml::from_str("producer='cargo-mutants'\nsources=['src/*.rs']").unwrap();
    let mut partition = Partition {
        name: "all".into(),
        config,
        sources: vec!["src/lib.rs".into()],
    };
    let mut options = EvidenceOptions {
        producer: Producer::CargoMutants,
        name: None,
        toolchain: None,
        timeout_secs: 30,
        args: vec![],
        producer_config: Some("all".into()),
    };
    validate(&options, &root, &partition).unwrap();
    for selector in ["--file=src/lib.rs", "--re=answer", "--shard=1/2"] {
        options.args = vec![selector.into()];
        assert!(validate(&options, &root, &partition).is_err());
        partition.config.scope = MutationScope::Sample;
        validate(&options, &root, &partition).unwrap();
        partition.config.scope = MutationScope::Exhaustive;
    }
    options.args = vec!["--all-features".into()];
    std::fs::create_dir(root.join(".cargo")).unwrap();
    let path = root.join(".cargo/mutants.toml");
    std::fs::write(&path, "jobs=1\nall_features=true\ntimeout=30").unwrap();
    validate(&options, &root, &partition).unwrap();
    for config in [
        "exclude_globs=['src/*.rs']",
        "test_package=['other']",
        "invalid = [",
    ] {
        std::fs::write(&path, config).unwrap();
        assert!(validate(&options, &root, &partition).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}
