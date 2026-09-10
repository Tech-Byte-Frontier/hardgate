use super::*;

#[test]
fn publication_failure_retains_workspace_and_cannot_issue_a_receipt() {
    let root = crate::fs_tests::tempdir("publication-failure");
    let source = root.join("source");
    let scratch = root.join("scratch");
    fs::create_dir_all(source.join(".hardgate/evidence")).unwrap();
    fs::create_dir(&scratch).unwrap();
    fs::write(source.join("main.ts"), "export const value = 1;\n").unwrap();
    let config = crate::config::HardgateConfig::default();
    let policy = inputs::InputPolicy::new(&source, &config).unwrap();
    let before = Snapshot::capture_with(&source, &policy).unwrap();
    let workspace = workspace::EvidenceWorkspace::create_at(&source, &scratch).unwrap();
    let job = workspace.job_path().to_path_buf();
    let report = workspace.root().join(".hardgate/evidence/result.lcov");
    fs::create_dir_all(report.parent().unwrap()).unwrap();
    fs::write(&report, "SF:main.ts\nDA:1,1\nLF:1\nLH:1\nend_of_record\n").unwrap();
    let destination = source.join(".hardgate/evidence/coverage.lcov");
    // A publication destination becomes unwritable after execution starts.
    let receipt = receipt_path(&destination);
    fs::create_dir(&receipt).unwrap();
    let result = publish(
        ProductionOutput {
            producer: Producer::Vitest,
            version: "fixture 1".into(),
            exit: 0,
            partition: None,
            runtime_inputs: None,
            spec: producer::CommandSpec {
                tokens: vec!["vitest".into(), "run".into()],
                version: vec![],
                prerequisite: None,
                auxiliary: vec![],
                report,
            },
        },
        Publication {
            root: &source,
            destination: &destination,
            before,
            workspace,
            config: &config,
            input_policy: policy,
        },
    );
    assert!(result.is_err());
    assert!(!receipt.is_file());
    assert!(job.join("work/main.ts").is_file());
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(job.join("lifecycle.json")).unwrap()).unwrap();
    assert_eq!(state["status"], "publication-failed");
    assert!(!job.join(".completed").exists());
    assert!(verify(&source, &destination, EvidenceKind::Coverage, &config).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn check_log_publication_rejects_files_and_symlinks_without_overwriting_them() {
    let root = crate::fs_tests::tempdir("check-log-destination");
    let source = root.join("source");
    let scratch = root.join("scratch");
    fs::create_dir_all(source.join(".hardgate/evidence")).unwrap();
    fs::create_dir(&scratch).unwrap();
    let destination = source.join(".hardgate/evidence/checks");
    let unrelated = root.join("unrelated");
    fs::write(&unrelated, "preserve").unwrap();
    for symlink in [false, true] {
        let workspace = workspace::EvidenceWorkspace::create_at(&source, &scratch).unwrap();
        workspace
            .diagnostics("test", "completed test output")
            .unwrap();
        if symlink {
            std::os::unix::fs::symlink(&unrelated, &destination).unwrap();
        } else {
            fs::write(&destination, "preserve").unwrap();
        }
        assert!(read_only_publication::publish(&workspace, &source).is_err());
        assert_eq!(fs::read_to_string(&destination).unwrap(), "preserve");
        drop(workspace);
        fs::remove_file(&destination).unwrap();
    }
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "preserve");
    fs::remove_dir_all(root).unwrap();
}
