use super::super::{
    FrameworkConfig, FrameworkConfigSearch, PackageManager, ResolvedTestPlan, TestFramework,
    TestSelection, ancestor_dirs, framework_without_script, is_javascript_path,
    resolve_js_test_plan, select_execution_root,
};
use crate::engines::mutation::js_manifest::{
    PackageMetadata, detect_manager, existing_metadata, find_workspace_root, manager_for_package,
    test_script, workspace_test_script,
};
use crate::engines::mutation::js_selection::test_support::{temp_root, write};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn metadata(root: &Path) -> PackageMetadata {
    let mut package = PackageMetadata::default();
    package.root = root.to_path_buf();
    package
}

fn framework_metadata(root: &Path, framework: Option<TestFramework>) -> PackageMetadata {
    let mut package = metadata(root);
    package.framework = framework;
    package
}

#[test]
fn resolver_value_types_cover_all_variants_and_timeout_paths() {
    for (manager, label) in [
        (PackageManager::Npm, "npm"),
        (PackageManager::Pnpm, "pnpm"),
        (PackageManager::Yarn, "yarn"),
        (PackageManager::Bun, "bun"),
    ] {
        assert_eq!(manager.as_str(), label);
    }
    for (framework, label, binary, args) in [
        (TestFramework::Jest, "jest", "jest", ""),
        (TestFramework::Vitest, "vitest", "vitest", "run"),
        (
            TestFramework::Playwright,
            "playwright",
            "playwright",
            "test",
        ),
    ] {
        assert_eq!(framework.as_str(), label);
        assert_eq!(framework.binary(), binary);
        assert_eq!(framework.args(), args);
    }

    let relevant = TestSelection::Relevant(PathBuf::from("tests/value.test.ts"));
    assert!(!relevant.is_full_suite());
    assert_eq!(
        relevant.relevant_test(),
        Some(Path::new("tests/value.test.ts"))
    );
    assert_eq!(relevant.description(), "relevant test");

    let full_suite = TestSelection::FullSuite;
    assert!(full_suite.is_full_suite());
    assert_eq!(full_suite.relevant_test(), None);
    assert_eq!(
        full_suite.description(),
        "full suite (no reliable test match)"
    );

    let custom = TestSelection::Custom;
    assert!(!custom.is_full_suite());
    assert_eq!(custom.relevant_test(), None);
    assert_eq!(custom.description(), "custom command");

    let mut plan = ResolvedTestPlan {
        command: String::new(),
        working_dir: PathBuf::new(),
        package_root: PathBuf::new(),
        workspace_root: PathBuf::new(),
        manager: None,
        framework: None,
        selection: TestSelection::FullSuite,
        recommended_timeout_secs: 60,
    };
    assert!(plan.full_suite_timeout_required());
    plan.selection = TestSelection::Custom;
    assert!(!plan.full_suite_timeout_required());
    plan.selection = TestSelection::FullSuite;
    plan.recommended_timeout_secs = 0;
    assert!(!plan.full_suite_timeout_required());
}

#[test]
fn resolver_rejects_non_directory_roots_and_source_directories() {
    let parent = temp_root("resolver-root-file-parent");
    let root_file = parent.join("root.txt");
    std::fs::write(&root_file, "not a repository").unwrap();
    let error = resolve_js_test_plan(Path::new("src/value.ts"), &root_file)
        .expect_err("a file cannot be used as the repository root");
    assert!(error.to_string().contains("repository root"));
    assert!(error.to_string().contains("not a directory"));
    let _ = std::fs::remove_dir_all(parent);

    let root = temp_root("resolver-source-directory");
    std::fs::create_dir_all(root.join("src/value.ts")).unwrap();
    let error = resolve_js_test_plan(&root.join("src/value.ts"), &root)
        .expect_err("a source directory must fail closed");
    assert!(error.to_string().contains("JavaScript source"));
    assert!(error.to_string().contains("not a file"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolver_framework_without_script_covers_variants() {
    let package_jest = metadata(Path::new("/package"));
    let package_vitest = framework_metadata(Path::new("/package"), Some(TestFramework::Vitest));
    let config_jest = FrameworkConfigSearch {
        selected: Some(FrameworkConfig {
            framework: TestFramework::Jest,
            root: PathBuf::from("/config"),
        }),
        ambiguous: false,
    };
    assert_eq!(
        framework_without_script(Some(&package_jest), &config_jest),
        Some(TestFramework::Jest)
    );
    assert_eq!(
        framework_without_script(Some(&package_vitest), &config_jest),
        None
    );
    assert_eq!(
        framework_without_script(
            Some(&framework_metadata(
                Path::new("/package"),
                Some(TestFramework::Jest),
            )),
            &config_jest,
        ),
        Some(TestFramework::Jest)
    );
    assert_eq!(
        framework_without_script(Some(&package_jest), &FrameworkConfigSearch::default()),
        None
    );
    let package_with_jest = framework_metadata(Path::new("/package"), Some(TestFramework::Jest));
    assert_eq!(
        framework_without_script(Some(&package_with_jest), &FrameworkConfigSearch::default()),
        Some(TestFramework::Jest)
    );
    assert_eq!(
        framework_without_script(None, &config_jest),
        Some(TestFramework::Jest)
    );
    assert_eq!(
        framework_without_script(None, &FrameworkConfigSearch::default()),
        None
    );
    assert_eq!(
        framework_without_script(
            Some(&{
                let mut package =
                    framework_metadata(Path::new("/package"), Some(TestFramework::Jest));
                package.framework_hint_ambiguous = true;
                package
            }),
            &config_jest,
        ),
        None
    );
    assert_eq!(
        framework_without_script(
            Some(&package_with_jest),
            &FrameworkConfigSearch {
                selected: None,
                ambiguous: true,
            },
        ),
        None
    );
}

#[test]
fn resolver_execution_roots_cover_optional_sources() {
    let package = metadata(Path::new("/package"));
    let config = FrameworkConfig {
        framework: TestFramework::Jest,
        root: PathBuf::from("/config"),
    };
    let fallback = Path::new("/fallback");
    assert_eq!(
        select_execution_root(Some(&package), Some(&config), true, fallback),
        PathBuf::from("/package")
    );
    assert_eq!(
        select_execution_root(None, Some(&config), true, fallback),
        PathBuf::from("/config")
    );
    assert_eq!(
        select_execution_root(None, None, true, fallback),
        PathBuf::from("/fallback")
    );
    assert_eq!(
        select_execution_root(Some(&package), Some(&config), false, fallback),
        PathBuf::from("/config")
    );
    assert_eq!(
        select_execution_root(Some(&package), None, false, fallback),
        PathBuf::from("/package")
    );
    assert_eq!(
        select_execution_root(None, None, false, fallback),
        PathBuf::from("/fallback")
    );
}

#[test]
fn resolver_ancestor_and_extension_helpers_cover_edges() {
    assert_eq!(
        ancestor_dirs(Path::new("root/src"), Path::new("root")),
        vec![PathBuf::from("root/src"), PathBuf::from("root")]
    );
    assert_eq!(
        ancestor_dirs(Path::new(""), Path::new("root")),
        vec![PathBuf::new()]
    );
    assert_eq!(
        ancestor_dirs(Path::new("/"), Path::new("/tmp")),
        vec![PathBuf::from("/")]
    );
    assert_eq!(
        ancestor_dirs(Path::new("/tmp/file"), Path::new("/root")),
        vec![PathBuf::from("/tmp/file"), PathBuf::from("/tmp")]
    );

    for extension in ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "JS"] {
        assert!(is_javascript_path(Path::new(&format!("value.{extension}"))));
    }
    for path in ["value", "value.rs", "value.js.txt", ".gitignore"] {
        assert!(!is_javascript_path(Path::new(path)), "{path}");
    }
}

#[test]
fn non_exact_bun_test_script_does_not_enable_selector() {
    let root = temp_root("bun-script-not-exact");
    write(
        &root,
        "package.json",
        r#"{"packageManager":"bun@1","scripts":{"test":"bun test --watch"}}"#,
    );
    write(&root, "src/value.ts", "export const value = true;\n");
    let plan = resolve_js_test_plan(&root.join("src/value.ts"), &root).unwrap();
    assert_eq!(plan.manager, Some(PackageManager::Bun));
    assert_eq!(plan.framework, None);
    assert_eq!(plan.selection, TestSelection::FullSuite);
    assert_eq!(plan.command, "bun run test");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn manifest_manager_detection_and_script_helpers_cover_fallbacks() {
    let explicit_root = temp_root("manager-explicit");
    let explicit = {
        let mut package = metadata(&explicit_root);
        package.package_manager = Some(PackageManager::Yarn);
        package
    };
    assert_eq!(manager_for_package(&explicit), Some(PackageManager::Yarn));
    let _ = std::fs::remove_dir_all(&explicit_root);

    let hinted_root = temp_root("manager-hinted");
    write(&hinted_root, "bun.lock", "");
    assert_eq!(
        manager_for_package(&metadata(&hinted_root)),
        Some(PackageManager::Bun)
    );
    let _ = std::fs::remove_dir_all(&hinted_root);

    let root = temp_root("manager-detection");
    let package = {
        let mut package = metadata(&root);
        package.package_manager = Some(PackageManager::Pnpm);
        package
    };
    assert_eq!(
        detect_manager(std::slice::from_ref(&root), std::slice::from_ref(&package)),
        PackageManager::Pnpm
    );
    write(&root, "pnpm-lock.yaml", "");
    assert_eq!(
        detect_manager(std::slice::from_ref(&root), &[]),
        PackageManager::Pnpm
    );
    std::fs::remove_file(root.join("pnpm-lock.yaml")).unwrap();
    assert_eq!(
        detect_manager(std::slice::from_ref(&root), &[]),
        PackageManager::Npm
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn manifest_test_script_helper_covers_script_shapes() {
    let root = temp_root("manifest-test-scripts");
    let mut scripts = BTreeMap::new();
    scripts.insert("test".to_string(), "jest".to_string());
    let package = {
        let mut package = metadata(&root);
        package.scripts = scripts;
        package
    };
    assert_eq!(
        test_script(&package).unwrap(),
        Some(("test".to_string(), "jest".to_string()))
    );
    let mut named = metadata(&root);
    named
        .scripts
        .insert("test:unit".to_string(), "vitest".to_string());
    assert_eq!(
        test_script(&named).unwrap(),
        Some(("test:unit".to_string(), "vitest".to_string()))
    );
    assert_eq!(test_script(&metadata(&root)).unwrap(), None);
    let mut multiple = metadata(&root);
    multiple
        .scripts
        .insert("test:unit".to_string(), "vitest".to_string());
    multiple
        .scripts
        .insert("test:e2e".to_string(), "playwright test".to_string());
    assert!(test_script(&multiple).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workspace_script_and_root_helpers_cover_missing_and_outside_packages() {
    let root = temp_root("workspace-script");
    let mut workspace = metadata(&root);
    workspace
        .scripts
        .insert("test".to_string(), "jest".to_string());
    let child = metadata(&root.join("packages/app"));
    let packages = vec![child.clone(), workspace.clone()];
    assert!(
        workspace_test_script(None, &packages, &root)
            .unwrap()
            .is_none()
    );
    assert!(
        workspace_test_script(Some(&workspace), &packages, &root)
            .unwrap()
            .is_none()
    );
    let (_, script) = workspace_test_script(Some(&child), &packages, &root)
        .unwrap()
        .expect("workspace test script");
    assert_eq!(script, ("test".to_string(), "jest".to_string()));
    assert!(
        workspace_test_script(Some(&child), std::slice::from_ref(&child), &root)
            .unwrap()
            .is_none()
    );
    let empty_workspace = metadata(&root.join("empty-workspace"));
    let packages = vec![child.clone(), empty_workspace];
    assert!(
        workspace_test_script(Some(&child), &packages, &root.join("empty-workspace"))
            .unwrap()
            .is_none()
    );
    let _ = std::fs::remove_dir_all(&root);

    let root = temp_root("workspace-root-package");
    write(&root, "pnpm-workspace.yaml", "packages:\n  - packages/*\n");
    let root_package = metadata(&root);
    assert_eq!(
        find_workspace_root(
            std::slice::from_ref(&root),
            std::slice::from_ref(&root_package),
            &root
        )
        .unwrap(),
        root
    );
    assert_eq!(
        find_workspace_root(std::slice::from_ref(&root), &[], &root.join("fallback")).unwrap(),
        root.join("fallback")
    );
    let _ = std::fs::remove_dir_all(&root);

    let workspace_root = temp_root("workspace-outside-root");
    write(
        &workspace_root,
        "pnpm-workspace.yaml",
        "packages:\n  - packages/*\n",
    );
    let outside = temp_root("workspace-outside-package");
    let outside_package = metadata(&outside);
    assert_eq!(
        find_workspace_root(
            std::slice::from_ref(&workspace_root),
            std::slice::from_ref(&outside_package),
            &workspace_root,
        )
        .unwrap(),
        outside
    );
    let _ = std::fs::remove_dir_all(workspace_root);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn workspace_patterns_and_metadata_report_non_not_found_errors() {
    super::manifest_error_case(
        "workspace-empty-negative",
        r#"{"workspaces":["packages/*","!"]}"#,
        "invalid workspace pattern",
    );
    super::manifest_error_case(
        "workspace-backslash",
        r#"{"workspaces":["packages/*","foo\\bar"]}"#,
        "invalid workspace pattern",
    );

    let fallback = temp_root("workspace-invalid-path-fallback");
    let error = find_workspace_root(&[PathBuf::from("\0")], &[], &fallback)
        .expect_err("invalid path inspection must fail closed");
    assert!(
        error
            .to_string()
            .contains("failed to inspect pnpm workspace file")
    );
    let error = existing_metadata(Path::new("\0"), "package manifest")
        .expect_err("invalid metadata path inspection must fail closed");
    assert!(
        error
            .to_string()
            .contains("failed to inspect package manifest")
    );
    let _ = std::fs::remove_dir_all(fallback);
}
