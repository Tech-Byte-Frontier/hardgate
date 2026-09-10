use super::*;

#[test]
fn installed_dependency_content_changes_invalidate_without_timestamp_assumptions() {
    let root = crate::fs_tests::tempdir("reuse-dependencies");
    fs::create_dir(root.join("package")).unwrap();
    fs::write(root.join("package/index.js"), "original").unwrap();
    let first = tree_digest(&root).unwrap();
    assert_eq!(first, tree_digest(&root).unwrap());
    fs::write(root.join("package/index.js"), "modified").unwrap();
    assert_ne!(first, tree_digest(&root).unwrap());
    fs::write(root.join("package/new.js"), "new").unwrap();
    let added = tree_digest(&root).unwrap();
    fs::remove_file(root.join("package/index.js")).unwrap();
    assert_ne!(added, tree_digest(&root).unwrap());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn rebased_internal_dependency_links_have_identical_content_identity() {
    let root = crate::fs_tests::tempdir("reuse-linked-dependencies");
    for directory in ["original", "copy"] {
        let base = root.join(directory);
        fs::create_dir_all(base.join(".pnpm/pkg")).unwrap();
        fs::write(base.join(".pnpm/pkg/index.js"), "original").unwrap();
    }
    std::os::unix::fs::symlink(".pnpm/pkg", root.join("original/pkg")).unwrap();
    std::os::unix::fs::symlink(root.join("copy/.pnpm/pkg"), root.join("copy/pkg")).unwrap();
    assert_eq!(
        tree_digest(&root.join("original")).unwrap(),
        tree_digest(&root.join("copy")).unwrap()
    );
    fs::write(root.join("original/pkg/index.js"), "changed").unwrap();
    assert_ne!(
        tree_digest(&root.join("original")).unwrap(),
        tree_digest(&root.join("copy")).unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn runtime_binding_covers_python_and_rust_installation_roots() {
    let root = crate::fs_tests::tempdir("runtime-installation-roots");
    assert!(!can_reuse(&root, Producer::Pytest));
    fs::create_dir_all(root.join(".venv/bin")).unwrap();
    fs::write(root.join(".venv/bin/python"), "bound interpreter").unwrap();
    let first = RuntimeInputs::capture(&root, Producer::Pytest).unwrap();
    first
        .require_same(&RuntimeInputs::capture(&root, Producer::Pytest).unwrap())
        .unwrap();
    fs::write(root.join(".venv/bin/python"), "changed interpreter").unwrap();
    assert!(
        first
            .require_same(&RuntimeInputs::capture(&root, Producer::Pytest).unwrap())
            .is_err()
    );
    assert_eq!(
        dependency_roots(&root, Producer::Pytest),
        vec![root.join(".venv"), root.join("venv")]
    );
    for producer in [Producer::CargoLlvmCov, Producer::CargoMutants] {
        let roots = dependency_roots(&root, producer);
        assert!(roots.iter().any(|path| path.ends_with("registry/src")));
        assert!(roots.iter().any(|path| path.ends_with("toolchains")));
        assert!(external_overrides(producer).contains(&"RUSTC_WRAPPER"));
    }
    assert!(external_overrides(Producer::Pytest).contains(&"PYTHONPATH"));
    assert!(external_overrides(Producer::Stryker).contains(&"NODE_OPTIONS"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn linked_interpreters_bind_bytes_and_special_dependency_inputs_are_rejected() {
    let root = crate::fs_tests::tempdir("runtime-linked-interpreter");
    let tree = root.join("venv");
    fs::create_dir(&tree).unwrap();
    let interpreter = root.join("python");
    fs::write(&interpreter, "first").unwrap();
    std::os::unix::fs::symlink(&interpreter, tree.join("python")).unwrap();
    let first = tree_digest(&tree).unwrap();
    fs::write(&interpreter, "second").unwrap();
    assert_ne!(first, tree_digest(&tree).unwrap());
    assert!(
        std::process::Command::new("mkfifo")
            .arg(tree.join("fifo"))
            .status()
            .unwrap()
            .success()
    );
    assert!(tree_digest(&tree).is_err());
    fs::remove_dir_all(root).unwrap();
}
