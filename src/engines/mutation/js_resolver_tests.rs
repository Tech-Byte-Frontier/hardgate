use super::super::js_selection::test_support::{temp_root, write};
use super::{PackageManager, TestSelection, resolve_js_test_plan};
use std::path::Path;

#[path = "../../../tests/support/js_resolver.rs"]
mod js_resolver_support;

fn workspace_error(root: &Path, reason: &str) -> String {
    resolve_js_test_plan(&root.join("src/value.ts"), root)
        .expect_err(reason)
        .to_string()
}

fn write_source(root: &Path) {
    write(root, "src/value.ts", "export const value = true;\n");
}

fn manifest_error_case(label: &str, manifest: &str, expected: &str) {
    let root = temp_root(label);
    write(&root, "package.json", manifest);
    write_source(&root);
    let message = workspace_error(&root, "invalid package metadata must fail closed");
    assert!(message.contains(expected), "{label}: {message}");
    let _ = std::fs::remove_dir_all(root);
}

fn resolver_error(root: &Path, expected: &str) -> String {
    let error = resolve_js_test_plan(&root.join("src/value.ts"), root)
        .expect_err("resolver metadata escape must fail closed");
    let message = error.to_string();
    assert!(message.contains(expected), "{message}");
    assert!(
        !message.contains("malformed JavaScript package manifest"),
        "{message}"
    );
    message
}

#[test]
fn source_escape_is_rejected_before_manifest_inspection() {
    js_resolver_support::assert_source_escape_rejected(temp_root, |source, root| {
        resolve_js_test_plan(source, root)
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
}

#[test]
fn source_without_package_keeps_npm_fallback() {
    let root = temp_root("source-no-package");
    write(&root, "src/value.ts", "export const value = true;\n");
    let plan = resolve_js_test_plan(&root.join("src/value.ts"), &root).unwrap();
    assert_eq!(plan.manager, Some(PackageManager::Npm));
    assert_eq!(plan.command, "npm test");
    assert_eq!(plan.package_root, root);
    assert_eq!(plan.workspace_root, root);
    assert_eq!(plan.working_dir, root);
    assert_eq!(plan.selection, TestSelection::FullSuite);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn package_manifest_directory_and_read_errors_fail_closed() {
    let directory = temp_root("manifest-directory");
    std::fs::create_dir_all(directory.join("package.json")).unwrap();
    write_source(&directory);
    let message = workspace_error(&directory, "a package manifest directory must fail closed");
    assert!(message.contains("not a regular file"), "{message}");
    let _ = std::fs::remove_dir_all(directory);

    let read_error = temp_root("manifest-invalid-utf8");
    std::fs::write(read_error.join("package.json"), [0xff_u8, 0xfe_u8]).unwrap();
    write_source(&read_error);
    let message = workspace_error(
        &read_error,
        "a package manifest read error must fail closed",
    );
    assert!(
        message.contains("failed to read JavaScript package manifest"),
        "{message}"
    );
    let _ = std::fs::remove_dir_all(read_error);
}

#[test]
fn scalar_package_manifest_fails_closed() {
    manifest_error_case("manifest-scalar", "null", "must contain a JSON object");
}

#[test]
fn package_scripts_require_object_and_string_values() {
    for (label, scripts, expected) in [
        ("scripts-array", "[]", "non-object `scripts` field"),
        (
            "scripts-non-string",
            r#"{"test":false}"#,
            "non-string script `test`",
        ),
    ] {
        manifest_error_case(
            label,
            &format!(r#"{{"packageManager":"npm@10","scripts":{scripts}}}"#),
            expected,
        );
    }
}

#[test]
fn package_manager_metadata_rejects_malformed_shapes() {
    for (label, manager, expected) in [
        (
            "manager-non-string",
            "false",
            "non-string `packageManager` field",
        ),
        (
            "manager-unsupported",
            r#""deno@2""#,
            "unsupported package manager",
        ),
    ] {
        manifest_error_case(
            label,
            &format!(r#"{{"packageManager":{manager}}}"#),
            expected,
        );
    }
}

#[test]
fn manifest_framework_hints_cover_jest_vitest_playwright_shapes() {
    use super::TestFramework;

    for (label, field, value, expected) in [
        (
            "manifest-jest-object",
            "jest",
            "{}",
            Some(TestFramework::Jest),
        ),
        (
            "manifest-vitest-object",
            "vitest",
            "{}",
            Some(TestFramework::Vitest),
        ),
        (
            "manifest-playwright-object",
            "playwright",
            "{}",
            Some(TestFramework::Playwright),
        ),
        ("manifest-jest-array", "jest", "[]", None),
        ("manifest-vitest-string", "vitest", r#""latest""#, None),
        ("manifest-playwright-null", "playwright", "null", None),
    ] {
        let root = temp_root(label);
        write(
            &root,
            "package.json",
            &format!(r#"{{"packageManager":"npm@10","{field}":{value}}}"#),
        );
        write_source(&root);
        write(&root, "tests/value.test.ts", "test('value', () => {});\n");
        let plan = resolve_js_test_plan(&root.join("src/value.ts"), &root).unwrap();
        assert_eq!(plan.framework, expected, "{label}");
        if let Some(framework) = expected {
            assert!(
                matches!(plan.selection, TestSelection::Relevant(_)),
                "{label}"
            );
            assert!(plan.command.contains(framework.as_str()), "{label}");
        } else {
            assert_eq!(plan.selection, TestSelection::FullSuite, "{label}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    let root = temp_root("manifest-dependencies-ignored");
    write(
        &root,
        "package.json",
        r#"{"packageManager":"npm@10","dependencies":{"jest":"30"},"devDependencies":{"vitest":"3","@playwright/test":"1"}}"#,
    );
    write_source(&root);
    let plan = resolve_js_test_plan(&root.join("src/value.ts"), &root).unwrap();
    assert_eq!(plan.framework, None);
    assert_eq!(plan.selection, TestSelection::FullSuite);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn metadata_symlinks_outside_root_are_rejected_before_read() {
    use std::os::unix::fs::symlink;
    let outside = temp_root("metadata-symlink-outside");
    write(&outside, "package.json", "{\n");
    write(&outside, "jest.config.js", "module.exports = {};\n");

    let package_root = temp_root("package-symlink-root");
    symlink(
        outside.join("package.json"),
        package_root.join("package.json"),
    )
    .unwrap();
    write_source(&package_root);
    let _ = resolver_error(&package_root, "package manifest");
    let _ = std::fs::remove_dir_all(package_root);

    let config_root = temp_root("config-symlink-root");
    write(
        &config_root,
        "package.json",
        r#"{"packageManager":"npm@10"}"#,
    );
    symlink(
        outside.join("jest.config.js"),
        config_root.join("jest.config.js"),
    )
    .unwrap();
    write_source(&config_root);
    let _ = resolver_error(&config_root, "framework config");
    let _ = std::fs::remove_dir_all(config_root);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn workspace_patterns_must_match_child_package() {
    for (label, child, manifest, pnpm, matched) in [
        (
            "package-match",
            "packages/app",
            r#"{"packageManager":"npm@10","workspaces":["packages/*"],"scripts":{"test":"node root.mjs"}}"#,
            None,
            true,
        ),
        (
            "package-nonmatch",
            "tools/app",
            r#"{"packageManager":"npm@10","workspaces":["packages/*"],"scripts":{"test":"node root.mjs"}}"#,
            None,
            false,
        ),
        (
            "package-excluded",
            "packages/app",
            r#"{"packageManager":"npm@10","workspaces":["packages/*","!packages/app"],"scripts":{"test":"node root.mjs"}}"#,
            None,
            false,
        ),
        (
            "pnpm-match",
            "packages/app",
            r#"{"packageManager":"pnpm@9","scripts":{"test":"node root.mjs"}}"#,
            Some("packages:\n  - packages/*\n"),
            true,
        ),
        (
            "pnpm-nonmatch",
            "tools/app",
            r#"{"packageManager":"pnpm@9","scripts":{"test":"node root.mjs"}}"#,
            Some("packages:\n  - packages/*\n"),
            false,
        ),
        (
            "pnpm-excluded",
            "packages/app",
            r#"{"packageManager":"pnpm@9","scripts":{"test":"node root.mjs"}}"#,
            Some("packages:\n  - packages/*\n  - '!packages/app'\n"),
            false,
        ),
    ] {
        let root = temp_root(label);
        write(&root, "package.json", manifest);
        if let Some(content) = pnpm {
            write(&root, "pnpm-workspace.yaml", content);
        }
        write(&root, &format!("{child}/package.json"), r#"{"name":"app"}"#);
        write(
            &root,
            &format!("{child}/src/value.ts"),
            "export const value = true;\n",
        );
        let value = resolve_js_test_plan(&root.join(child).join("src/value.ts"), &root).unwrap();
        let expected_root = if matched {
            root.clone()
        } else {
            root.join(child)
        };
        assert_eq!(value.workspace_root, expected_root, "{label}");
        assert_eq!(value.working_dir, expected_root, "{label}");
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn workspace_pattern_escape_fails_closed() {
    for (label, manifest, pnpm) in [
        ("package-escape", r#"{"workspaces":["../*"]}"#, None),
        (
            "pnpm-escape",
            r#"{"packageManager":"pnpm@9"}"#,
            Some("packages:\n  - ../*\n"),
        ),
    ] {
        let root = temp_root(label);
        write(&root, "package.json", manifest);
        if let Some(content) = pnpm {
            write(&root, "pnpm-workspace.yaml", content);
        }
        write_source(&root);
        let message = workspace_error(
            &root,
            "workspace patterns escaping the root must fail closed",
        );
        assert!(message.contains("workspace"));
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn all_negative_workspace_patterns_fail_closed() {
    for (label, workspaces) in [
        ("package-all-negative", r#"["!packages/*"]"#),
        (
            "package-object-all-negative",
            r#"{"packages":["!packages/*"]}"#,
        ),
    ] {
        manifest_error_case(
            label,
            &format!(r#"{{"workspaces":{workspaces}}}"#),
            "invalid workspace pattern",
        );
    }
}

#[test]
fn workspace_root_can_be_root_package_and_outside_packages_fall_back() {
    let root = temp_root("workspace-root-package");
    write(
        &root,
        "package.json",
        r#"{"packageManager":"npm@10","workspaces":["packages/*"],"scripts":{"test":"node root.mjs"}}"#,
    );
    write_source(&root);
    let plan = resolve_js_test_plan(&root.join("src/value.ts"), &root).unwrap();
    assert_eq!(plan.workspace_root, root);
    assert_eq!(plan.working_dir, root);
    assert_eq!(plan.command, "npm test");
    let _ = std::fs::remove_dir_all(root);

    let root = temp_root("workspace-outside-package");
    write(
        &root,
        "package.json",
        r#"{"packageManager":"npm@10","workspaces":["packages/*"],"scripts":{"test":"node root.mjs"}}"#,
    );
    write(&root, "tools/app/package.json", r#"{"name":"app"}"#);
    write(
        &root,
        "tools/app/src/value.ts",
        "export const value = true;\n",
    );
    let plan = resolve_js_test_plan(&root.join("tools/app/src/value.ts"), &root).unwrap();
    assert_eq!(plan.workspace_root, root.join("tools/app"));
    assert_eq!(plan.working_dir, root.join("tools/app"));
    assert_eq!(plan.command, "npm test");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn pnpm_workspace_file_errors_are_propagated() {
    for (label, content) in [
        ("pnpm-malformed", "packages: [\n"),
        ("pnpm-invalid-shape", "packages: {name: app}\n"),
        ("pnpm-empty-patterns", "packages: []\n"),
        ("pnpm-whitespace-pattern", "packages:\n  - '   '\n"),
    ] {
        let root = temp_root(label);
        write(&root, "package.json", r#"{"packageManager":"pnpm@9"}"#);
        write(&root, "pnpm-workspace.yaml", content);
        write(&root, "src/value.ts", "export const value = true;\n");
        let message = workspace_error(&root, "invalid pnpm workspace must fail closed");
        assert!(message.contains("pnpm workspace file"), "{message}");
        assert!(message.contains("pnpm-workspace.yaml"), "{message}");
        let _ = std::fs::remove_dir_all(root);
    }

    let root = temp_root("pnpm-unreadable");
    write(&root, "package.json", r#"{"packageManager":"pnpm@9"}"#);
    std::fs::create_dir_all(root.join("pnpm-workspace.yaml")).unwrap();
    write(&root, "src/value.ts", "export const value = true;\n");
    let message = workspace_error(&root, "non-file pnpm workspace must fail closed");
    assert!(message.contains("not a regular file"));
    let _ = std::fs::remove_dir_all(root);
}

#[path = "js_resolver_gap_tests.rs"]
mod gap_tests;
