use super::*;
use serde_json::json;

#[test]
fn exhaustive_stryker_partitions_require_native_inventory_including_zero_mutant_files() {
    let root = crate::fs_tests::tempdir("partition-native-inventory");
    std::fs::write(
        root.join("answer.js"),
        "export function answer() { return 42; }",
    )
    .unwrap();
    std::fs::create_dir_all(root.join(".hardgate/evidence")).unwrap();
    let report = root.join(".hardgate/evidence/mutation.json");
    let mut config = HardgateConfig::default();
    let producer: crate::config::ProducerConfig =
        toml::from_str("producer='stryker'\nsources=['*.js']").unwrap();
    config
        .evidence
        .producers
        .insert("all".into(), producer.clone());
    let mut partition = Partition {
        name: "all".into(),
        config: producer,
        sources: vec!["answer.js".into()],
    };
    let mut value = json!({"framework":{"version":"10.0.0"},"files":{"answer.js":{"mutants":[]}}});
    std::fs::write(&report, value.to_string()).unwrap();
    assert!(
        validate_partition_report(Producer::Stryker, &report, (&root, &config), &partition)
            .is_err()
    );
    config.evidence.producers.get_mut("all").unwrap().scope = MutationScope::Sample;
    partition.config.scope = MutationScope::Sample;
    validate_partition_report(Producer::Stryker, &report, (&root, &config), &partition).unwrap();
    config.evidence.producers.get_mut("all").unwrap().scope = MutationScope::Exhaustive;
    partition.config.scope = MutationScope::Exhaustive;
    value["hardgate_scope"] = json!({"schema_version":1,"producer_version":"10.0.0","baseline_passed":true,"completed":true,"sources":super::super::Snapshot::capture(&root).unwrap().0,"mutants":[]});
    std::fs::write(&report, value.to_string()).unwrap();
    validate_partition_report(Producer::Stryker, &report, (&root, &config), &partition).unwrap();
    std::fs::write(root.join("zero.js"), "export const zero = 0;").unwrap();
    assert!(
        validate_partition_report(Producer::Stryker, &report, (&root, &config), &partition)
            .is_err()
    );
    partition.sources.push("zero.js".into());
    assert!(
        validate_partition_report(Producer::Stryker, &report, (&root, &config), &partition)
            .is_err()
    );
    value["hardgate_scope"]["sources"] =
        serde_json::to_value(super::super::Snapshot::capture(&root).unwrap().0).unwrap();
    std::fs::write(&report, value.to_string()).unwrap();
    validate_partition_report(Producer::Stryker, &report, (&root, &config), &partition).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rust_mutation_inventory_ignores_baselines_but_requires_each_mutant_source() {
    let root = crate::fs_tests::tempdir("rust-reported-inventory");
    let report = root.join("outcomes.json");
    let config = HardgateConfig::default();
    let mut value =
        json!({"outcomes":[{"scenario":"Baseline"},{"scenario":{"Mutant":{"file":"src/lib.rs"}}}]});
    std::fs::write(&report, value.to_string()).unwrap();
    assert_eq!(
        reported_sources(Producer::CargoMutants, &report, &root, &config).unwrap(),
        BTreeSet::from([PathBuf::from("src/lib.rs")])
    );
    value["outcomes"][1]["scenario"]["Mutant"]["file"] = json!(42);
    std::fs::write(&report, value.to_string()).unwrap();
    assert!(reported_sources(Producer::CargoMutants, &report, &root, &config).is_err());
    std::fs::write(&report, "{}").unwrap();
    assert!(reported_sources(Producer::CargoMutants, &report, &root, &config).is_err());
    assert!(reported_sources(Producer::Stryker, &report, &root, &config).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
