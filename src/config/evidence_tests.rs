use super::*;

#[test]
fn named_producers_reject_empty_negative_and_escaping_configuration() {
    let base = "[producers.frontend]\nproducer='vitest'\nsources=['./src/**/*.ts']\nconfig='./vitest.config.ts'\n";
    toml::from_str::<EvidenceConfig>(base)
        .unwrap()
        .validate()
        .unwrap();
    for invalid in [
        base.replace("frontend", "'bad name'"),
        base.replace("./src/**/*.ts", ""),
        base.replace("./src/**/*.ts", "!src/**"),
        base.replace("./src/**/*.ts", "/outside/**"),
        base.replace("./src/**/*.ts", "../outside/**"),
        base.replace("./src/**/*.ts", "["),
        base.replace("./vitest.config.ts", "../vitest.config.ts"),
        base.replace("./vitest.config.ts", "/vitest.config.ts"),
        format!("{base}timeout_secs=0\n"),
        base.replace("['./src/**/*.ts']", "[]"),
    ] {
        assert!(
            toml::from_str::<EvidenceConfig>(&invalid)
                .unwrap()
                .validate()
                .is_err(),
            "accepted {invalid}"
        );
    }
}
