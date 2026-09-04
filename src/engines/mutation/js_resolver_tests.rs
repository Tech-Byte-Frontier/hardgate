use super::super::js_tests::test_support::{temp_root, write};
use super::{PackageManager, TestSelection, resolve_js_test_plan};
use std::path::Path;

fn workspace_error(root: &Path, reason: &str) -> String {
    resolve_js_test_plan(&root.join("src/value.ts"), root)
        .expect_err(reason)
        .to_string()
}

fn write_source(root: &Path) {
    write(root, "src/value.ts", "export const value = true;\n");
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
    let root = temp_root("source-root");
    write(&root, "package.json", r#"{"packageManager":"npm@10"}"#);
    let outside = temp_root("source-outside");
    write(&outside, "package.json", "{\n");
    write(&outside, "src/value.ts", "export const value = true;\n");
    let check = |source: &Path| {
        let error = resolve_js_test_plan(source, &root)
            .expect_err("external absolute source must fail closed");
        let message = error.to_string();
        assert!(message.contains("outside repository root"), "{message}");
        assert!(
            !message.contains("malformed JavaScript package manifest"),
            "{message}"
        );
    };
    check(&outside.join("src/value.ts"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        std::fs::create_dir_all(root.join("src")).unwrap();
        symlink(outside.join("src/value.ts"), root.join("src/escape.ts")).unwrap();
        check(&root.join("src/escape.ts"));
    }
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
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
        let expected_root = matched
            .then_some(root.clone())
            .unwrap_or_else(|| root.join(child));
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
fn pnpm_workspace_file_errors_are_propagated() {
    for (label, content) in [
        ("pnpm-malformed", "packages: [\n"),
        ("pnpm-invalid-shape", "packages: {name: app}\n"),
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
