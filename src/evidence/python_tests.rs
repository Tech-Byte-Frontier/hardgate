use super::*;
use crate::config::{MutationScope, ProducerConfig};
use crate::evidence::Producer;

fn options() -> EvidenceOptions {
    EvidenceOptions {
        producer: Producer::Pytest,
        name: None,
        toolchain: None,
        timeout_secs: 30,
        args: vec!["tests".into()],
        producer_config: None,
    }
}

fn partition() -> Partition {
    Partition {
        name: "python".into(),
        config: ProducerConfig {
            producer: Producer::Pytest,
            config: Some("pytest.ini".into()),
            sources: vec!["src/*.py".into()],
            scope: MutationScope::Exhaustive,
            toolchain: None,
            args: vec![],
            timeout_secs: 30,
        },
        sources: vec!["src/module.py".into()],
    }
}

#[test]
fn python_commands_use_exact_sources_branch_coverage_and_native_json() {
    let root = crate::fs_tests::tempdir("python-producer");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("output")).unwrap();
    std::fs::write(root.join("src/module.py"), "def answer():\n    return 42\n").unwrap();
    std::fs::write(root.join("pytest.ini"), "[pytest]\n").unwrap();
    let output = root.join("output");
    let selected = partition();
    let spec = prepare(&options(), &root, output.clone(), Some(&selected)).unwrap();
    let prerequisite = spec.prerequisite.unwrap();
    assert!(
        prerequisite
            .windows(2)
            .any(|pair| pair == ["-c", "pytest.ini"])
    );
    assert!(prerequisite.contains(&"--branch".into()));
    assert_eq!(prerequisite.last().unwrap(), "tests");
    assert!(spec.tokens.contains(&"lcov".into()));
    assert!(spec.auxiliary[0].contains(&"json".into()));
    let config = std::fs::read_to_string(output.join("coverage.ini")).unwrap();
    assert!(config.contains(&root.join("src/module.py").display().to_string()));
    assert!(config.contains("exclude_lines =\nexclude_also =\npartial_branches =\n"));
    let all = prepare(&options(), &root, output.clone(), None).unwrap();
    assert_eq!(all.version[0], "python3");
    std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
    std::fs::write(root.join(".venv/bin/python"), "installed runtime").unwrap();
    let local = prepare(&options(), &root, output, Some(&selected)).unwrap();
    assert_eq!(
        local.version[0],
        root.join(".venv/bin/python").display().to_string()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn python_rejects_scope_escape_and_nonexecuting_arguments() {
    let root = crate::fs_tests::tempdir("python-producer-rejections");
    let output = root.join("output");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(root.join("pytest.ini"), "[pytest]\n").unwrap();
    let mut selected = partition();
    for argument in ["--collect-only", "../outside", "tests/test_a.py::case"] {
        let mut request = options();
        request.args = vec![argument.into()];
        assert!(prepare(&request, &root, output.clone(), Some(&selected)).is_err());
    }
    let mut request = options();
    request.toolchain = Some("stable".into());
    assert!(prepare(&request, &root, output.clone(), Some(&selected)).is_err());
    for path in ["../pytest.ini", "absent.ini", "node_modules/config.ini"] {
        selected.config.config = Some(path.into());
        assert!(prepare(&options(), &root, output.clone(), Some(&selected)).is_err());
    }
    selected.sources = vec!["index.js".into()];
    assert!(prepare(&options(), &root, output.clone(), Some(&selected)).is_err());
    assert!(prepare(&options(), &root, output, None).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
