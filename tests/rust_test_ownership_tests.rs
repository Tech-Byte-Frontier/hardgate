use hardgate::commands::run_static_gate_snapshot;
use hardgate::config::HardgateConfig;
use std::path::PathBuf;

fn policy() -> HardgateConfig {
    let mut config = HardgateConfig::default();
    hardgate::config::Preset::Balanced.apply_to(&mut config);
    config.budgets.functions.max_parameters = Some(1);
    config.roles.test.max_parameters = Some(3);
    config.roles.source.max_lines = Some(4);
    config.roles.test.max_lines = Some(1000);
    config
}

#[test]
fn cfg_logic_requires_positive_test_ownership_and_bounds_uncertain_predicates() {
    let predicates = [
        ("#[cfg(all(test, true))]", true),
        ("#[cfg(any(test, false))]", true),
        ("#[cfg(all(test, unix))]", true),
        ("#[cfg(any(test, unix))]", false),
        ("#[cfg(not(not(test)))]", true),
        ("#[cfg(not(test))]", false),
        ("#[cfg(all(test, not(test)))]", false),
        ("#[cfg(all())]", false),
        ("#[cfg(any())]", false),
        ("#[cfg(not())]", false),
        ("#[cfg(not(test, true))]", false),
        ("#[cfg(unknown(test))]", false),
        ("#[cfg(test, unix)]", false),
        ("#[cfg[test]]", false),
        ("#[cfg]", false),
        ("#[custom]", false),
        ("#[test]", true),
        ("#[bench]", true),
        (
            "#[cfg(all(/* explanation */ test, feature = r#\"a,b\"#))]",
            true,
        ),
    ];
    for (attribute, expected) in predicates {
        let source = format!("{attribute}\nfn candidate() {{}}\n");
        let report = analyze(&policy(), &[("lib.rs", &source)]);
        let candidate = report
            .functions
            .iter()
            .find(|function| function.name == "candidate")
            .unwrap();
        assert_eq!(candidate.test_only, expected, "{attribute}: {report:?}");
    }
    let nested = format!(
        "#[cfg({}test{})]\nfn bounded() {{}}\n",
        "not(".repeat(66),
        ")".repeat(66)
    );
    let report = analyze(&policy(), &[("lib.rs", &nested)]);
    assert!(!report.functions[0].test_only);
}

#[test]
fn declared_named_targets_override_test_imports_even_with_auto_discovery_disabled() {
    for (section, directory, automatic, expected) in [
        ("bin", "src/bin", "autobins", false),
        ("example", "examples", "autoexamples", false),
        ("test", "tests", "autotests", true),
        ("bench", "benches", "autobenches", true),
    ] {
        for suffix in ["demo.rs", "demo/main.rs"] {
            let path = format!("{directory}/{suffix}");
            let manifest = format!(
                "[package]\nname='layout'\nversion='0.1.0'\n{automatic}=false\n[lib]\npath='lib.rs'\n[[{section}]]\nname='demo'\n"
            );
            let library = format!("#[cfg(test)]\n#[path = r#\"{path}\"#]\nmod imported;\n");
            let report = analyze(
                &policy(),
                &[
                    ("Cargo.toml", &manifest),
                    ("lib.rs", &library),
                    (&path, "pub fn candidate() {}\n"),
                ],
            );
            let candidate = report
                .functions
                .iter()
                .find(|function| function.name == "candidate")
                .unwrap();
            assert_eq!(
                candidate.test_only, expected,
                "{section}/{suffix}: {report:?}"
            );
        }
    }
}

#[test]
fn custom_build_scripts_and_implicit_library_paths_remain_production_roots() {
    let report = analyze(
        &policy(),
        &[
            (
                "Cargo.toml",
                "[package]\nname='layout'\nversion='0.1.0'\nbuild='compile.rs'\n[lib]\nname='layout'\n",
            ),
            (
                "src/lib.rs",
                "#[cfg(test)]\n#[path=\"../compile.rs\"]\nmod helper;\n",
            ),
            ("compile.rs", "fn compile() {}\n"),
            ("examples/no_extension", "ignored"),
            ("examples/demo/readme.txt", "ignored"),
        ],
    );
    assert!(
        !report
            .functions
            .iter()
            .find(|function| function.name == "compile")
            .unwrap()
            .test_only
    );
}

fn analyze(config: &HardgateConfig, files: &[(&str, &str)]) -> hardgate::GateReport {
    let files = files
        .iter()
        .map(|(name, text)| (PathBuf::from(name), text.to_string()))
        .collect::<Vec<_>>();
    let (mut report, files, _, functions) = run_static_gate_snapshot(config, &files).unwrap();
    report.functions = functions;
    report.finalize(files.len(), report.functions.len(), 0);
    report
}

#[test]
fn inline_cfg_test_portions_keep_test_budgets_and_original_locations() {
    let source = "pub fn production(a: i32, b: i32) -> i32 { a + b }\n#[cfg(test)]\nmod specs {\n    #[test]\n    fn check() { assert_eq!(helper(1, 2), 3); }\n    fn helper(a: i32, b: i32) -> i32 { a + b }\n    macro_rules! sample { () => { 1 + 2 }; }\n}\n";
    let report = analyze(&policy(), &[("src/lib.rs", source)]);
    assert!(report.budget_violations.is_empty(), "{report:?}");
    assert_eq!(report.complexity_violations.len(), 1, "{report:?}");
    assert_eq!(report.complexity_violations[0].function_name, "production");
    let helper = report
        .functions
        .iter()
        .find(|function| function.name == "helper")
        .unwrap();
    assert!(helper.test_only);
    assert_eq!(helper.start_line, 6);
}

#[test]
fn test_only_module_ownership_follows_path_attributes_and_retains_shared_production_use() {
    let library = "#[cfg(test)]\n#[path = \"support.rs\"]\nmod support;\n";
    let helper = "pub fn helper(a: i32, b: i32) -> i32 {\n    let first = a + 1;\n    let second = b + 2;\n    first + second\n}\n";
    let report = analyze(
        &policy(),
        &[("src/lib.rs", library), ("src/support.rs", helper)],
    );
    assert!(report.passed, "{report:?}");
    assert!(report.functions.iter().all(|function| function.test_only));
    let shared = analyze(
        &policy(),
        &[
            ("src/lib.rs", library),
            ("src/main.rs", "mod support;\nfn main() {}\n"),
            ("src/support.rs", helper),
        ],
    );
    assert!(
        shared
            .complexity_violations
            .iter()
            .any(|finding| finding.function_name == "helper")
    );
    assert!(
        shared
            .functions
            .iter()
            .any(|function| function.name == "helper" && !function.test_only),
        "{report:?}"
    );
    assert!(
        shared
            .budget_violations
            .iter()
            .any(|finding| finding.file == std::path::Path::new("src/support.rs"))
    );
}

#[test]
fn cfg_boolean_logic_requires_test_but_feature_alternatives_remain_production() {
    for (condition, testing) in [
        ("all(test, feature = \"extended\")", true),
        ("any(test, feature = \"extended\")", false),
        ("not(not(test))", true),
        ("not(test)", false),
    ] {
        let source = format!("#[cfg({condition})]\nfn helper(a: i32, b: i32) -> i32 {{ a + b }}\n");
        let report = analyze(&policy(), &[("src/lib.rs", &source)]);
        assert_eq!(
            report.functions[0].test_only, testing,
            "{condition}: {report:?}"
        );
        assert_eq!(report.complexity_violations.is_empty(), testing);
    }
}

#[test]
fn a_declared_cargo_binary_is_not_test_only_even_when_imported_under_cfg_test() {
    let report = analyze(
        &policy(),
        &[
            (
                "Cargo.toml",
                "[package]\nname='fixture'\nversion='0.1.0'\n[[bin]]\nname='fuzz-driver'\npath='src/support.rs'\n",
            ),
            ("src/lib.rs", "#[cfg(test)]\nmod support;\n"),
            (
                "src/support.rs",
                "fn helper(a: i32, b: i32) -> i32 { a + b }\nfn main() {}\n",
            ),
        ],
    );
    assert!(
        report
            .functions
            .iter()
            .any(|function| function.name == "helper" && !function.test_only)
    );
    assert!(
        report
            .complexity_violations
            .iter()
            .any(|finding| finding.function_name == "helper")
    );
}

#[test]
fn duplicate_cfg_test_macros_are_advisories_in_the_test_clone_group() {
    let mut config = policy();
    config.roles.source.max_lines = Some(1000);
    config.clones.min_lines = 3;
    config.clones.min_tokens = 15;
    config.roles.test.clone_min_lines = Some(3);
    config.roles.test.clone_min_tokens = Some(15);
    let source = "#[cfg(test)]\nmacro_rules! fixture {\n    ($value:expr) => {{\n        let first = $value + 1;\n        let second = first * 2;\n        let third = second - 3;\n        third + 10\n    }};\n}\n";
    let report = analyze(
        &config,
        &[("src/first.rs", source), ("src/second.rs", source)],
    );
    assert!(report.clone_violations.is_empty(), "{report:?}");
    assert!(
        report
            .advisories
            .iter()
            .any(|note| note.contains("Test") && note.contains("clone")),
        "{report:?}"
    );
}

#[test]
fn inner_test_attributes_and_nested_external_module_paths_propagate() {
    let report = analyze(
        &policy(),
        &[
            ("src/lib.rs", "mod feature;\n"),
            (
                "src/feature.rs",
                "mod nested {\n#![cfg(test)]\nmod helper;\n}\n",
            ),
            (
                "src/feature/nested/helper.rs",
                "pub fn helper(a: i32, b: i32) -> i32 { a + b }\n",
            ),
        ],
    );
    assert!(
        report
            .functions
            .iter()
            .find(|function| function.name == "helper")
            .unwrap()
            .test_only,
        "{report:?}"
    );
}

#[path = "support/fs.rs"]
mod fs_tests;

#[test]
fn scoped_gate_and_single_file_scan_resolve_test_ownership_without_hiding_excluded_importers() {
    let root = fs_tests::tempdir("rust-ownership-scope");
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "#[cfg(test)]\nmod support;\n").unwrap();
    let helper = "pub fn helper(a: i32, b: i32) -> i32 { a + b }\n";
    let target = root.join("src/support.rs");
    std::fs::write(&target, helper).unwrap();
    let mut config = policy();
    let analyze_scoped = |config: &HardgateConfig| {
        let (_, _, _, functions) = hardgate::commands::run_static_gate_at(
            config,
            false,
            std::slice::from_ref(&target),
            &root,
        )
        .unwrap()
        .unwrap();
        functions[0].test_only
    };
    assert!(analyze_scoped(&config));
    let scanner = hardgate::engines::AntiGamingScanner::new(&config.anti_gaming);
    let invariants = hardgate::engines::InvariantsChecker::new(&config.invariants.rules);
    let mut report = hardgate::GateReport::new("scan".into());
    let functions = hardgate::commands::analyze_file_content(
        hardgate::commands::AnalyzeInput {
            path: &target,
            content: helper,
            config: &config,
            root: &root,
            anti_gaming: &scanner,
            invariants: &invariants,
        },
        &mut report,
    );
    assert!(functions[0].test_only, "{report:?}");
    std::fs::write(root.join("src/main.rs"), "mod support;\nfn main() {}\n").unwrap();
    config
        .budgets
        .files
        .exclusions
        .paths
        .push("src/main.rs".into());
    assert!(!analyze_scoped(&config));
    let (_, _, _, functions) = hardgate::commands::run_static_gate_at(&config, false, &[], &root)
        .unwrap()
        .unwrap();
    assert!(
        !functions
            .iter()
            .find(|function| function.name == "helper")
            .unwrap()
            .test_only
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn automatic_binary_roots_and_uncertain_macro_modules_cannot_be_claimed_as_test_only() {
    let library = "#[cfg(test)]\n#[path=\"bin/tool.rs\"]\nmod tool;\n";
    for (automatic, testing) in [(true, false), (false, true)] {
        let manifest =
            format!("[package]\nname='fixture'\nversion='0.1.0'\nautobins={automatic}\n");
        let report = analyze(
            &policy(),
            &[
                ("Cargo.toml", &manifest),
                ("src/lib.rs", library),
                (
                    "src/bin/tool.rs",
                    "fn helper(a: i32, b: i32) -> i32 { a + b }\nfn main() {}\n",
                ),
            ],
        );
        assert_eq!(
            report
                .functions
                .iter()
                .find(|function| function.name == "helper")
                .unwrap()
                .test_only,
            testing,
            "{report:?}"
        );
    }
    for production in [
        "include!(concat!(\"sup\", \"port.rs\"));\n",
        "macro_rules! declare { () => { mod support; }; }\ndeclare!();\n",
    ] {
        let report = analyze(
            &policy(),
            &[
                ("src/lib.rs", "#[cfg(test)]\nmod support;\n"),
                ("src/main.rs", production),
                (
                    "src/support.rs",
                    "fn helper(a: i32, b: i32) -> i32 { a + b }\n",
                ),
            ],
        );
        assert!(
            !report
                .functions
                .iter()
                .find(|function| function.name == "helper")
                .unwrap()
                .test_only,
            "{report:?}"
        );
    }
}

#[test]
fn cfg_test_fields_statements_and_methods_use_original_syntax() {
    let source = r#"pub struct Context {
    #[cfg(test)]
    marker: bool,
    value: bool,
}
impl Context {
    pub fn make(value: bool) -> Self {
        #[cfg(test)]
        if value { if value { panic!("test-only branch"); } }
        Self {
            #[cfg(test)]
            marker: value,
            value,
        }
    }
    #[cfg(test)]
    fn inspect(&self) -> bool { if self.marker { true } else { false } }
    pub fn value(&self) -> bool { self.value }
}
"#;
    let config = hardgate::config::Preset::Balanced.to_default_config();
    let report = analyze(&config, &[("src/lib.rs", source)]);
    assert!(report.orchestration_violations.is_empty(), "{report:?}");
    assert_eq!(report.functions.len(), 3);
    let make = report
        .functions
        .iter()
        .find(|item| item.name == "make")
        .unwrap();
    assert!(!make.test_only);
    assert_eq!(make.cyclomatic, 1);
    assert_eq!(make.max_nesting_depth, 0);
    assert_eq!(make.lines, 5);
    let inspect = report
        .functions
        .iter()
        .find(|item| item.name == "inspect")
        .unwrap();
    assert!(inspect.test_only);
    assert_eq!(inspect.cyclomatic, 2);
    let value = report
        .functions
        .iter()
        .find(|item| item.name == "value")
        .unwrap();
    assert!(!value.test_only);
    assert_eq!(value.start_line, 18);
}
