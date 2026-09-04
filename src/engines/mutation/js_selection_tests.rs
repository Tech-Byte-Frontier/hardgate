use super::super::js::{
    PackageManager, ResolvedTestPlan, TestFramework, TestSelection, resolve_js_test_plan,
};
use super::super::js_manifest::valid_pnpm_workspace_content;
use super::test_support::{temp_root, write, write_workspace_fixture};
use std::path::Path;

fn plan(root: &Path) -> ResolvedTestPlan {
    resolve_js_test_plan(&root.join("src/value.ts"), root).unwrap()
}
fn script_plan(root: &Path, manager: &str, script: &str) -> ResolvedTestPlan {
    write_package(
        root,
        &format!(r#"{{"packageManager":"{manager}","scripts":{{"test":"{script}"}}}}"#),
    );
    write(root, "src/value.ts", "export const value = true;\n");
    write(root, "tests/value.test.ts", "test('value', () => {});\n");
    plan(root)
}
fn write_app(root: &Path, file: &str, content: &str) {
    write(root, &format!("packages/app/{file}"), content);
}
fn write_package(root: &Path, content: &str) {
    write(root, "package.json", content);
}
fn assert_full_suite(value: &ResolvedTestPlan, label: &str) {
    assert_eq!(value.framework, None, "{label}");
    assert_eq!(value.selection, TestSelection::FullSuite, "{label}");
    assert_eq!(value.recommended_timeout_secs, 60, "{label}");
    assert!(!value.command.contains("value.test.ts"), "{label}");
}
#[test]
fn lockfile_only_nested_package_is_not_a_workspace_boundary() {
    let root = temp_root("lockfile");
    write_package(&root, r#"{"packageManager":"npm@10"}"#);
    write(
        &root,
        "packages/app/package.json",
        r#"{"name":"app","scripts":{"test":"node scripts/test.mjs"}}"#,
    );
    write(&root, "packages/app/pnpm-lock.yaml", "lockfileVersion: 9\n");
    write_app(&root, "src/value.ts", "export const value = true;\n");
    write_app(&root, "tests/value.test.ts", "test('value', () => {});\n");
    let value = resolve_js_test_plan(&root.join("packages/app/src/value.ts"), &root).unwrap();
    assert_eq!(value.package_root, root.join("packages/app"));
    assert_eq!(value.workspace_root, root.join("packages/app"));
    assert_eq!(value.manager, Some(PackageManager::Pnpm));
    assert_eq!(value.selection, TestSelection::FullSuite);
    assert!(!value.command.contains("value.test.ts"));
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn malformed_nearest_manifest_returns_explicit_error() {
    let root = temp_root("malformed-manifest");
    write_package(&root, r#"{"packageManager":"bun@1"}"#);
    write(&root, "packages/app/package.json", "{\"name\":\"app\",\n");
    write_app(&root, "src/value.ts", "export const value = true;\n");
    let error = resolve_js_test_plan(&root.join("packages/app/src/value.ts"), &root)
        .expect_err("nearest malformed package must fail closed");
    let message = error.to_string();
    assert!(message.contains("malformed JavaScript package manifest"));
    assert!(message.contains("packages/app/package.json"));
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn invalid_package_workspaces_shapes_fail_closed() {
    for (label, workspaces) in [
        ("boolean", "true"),
        ("null", "null"),
        ("empty-array", "[]"),
        ("object-missing", "{}"),
        ("object-string", r#"{"packages":"packages/*"}"#),
        ("object-empty", r#"{"packages":[]}"#),
        ("array-number", "[1]"),
    ] {
        let root = temp_root(label);
        write_package(
            &root,
            &format!(r#"{{"packageManager":"pnpm@9","workspaces":{workspaces}}}"#),
        );
        write(&root, "packages/app/package.json", r#"{"name":"app"}"#);
        write_app(&root, "src/value.ts", "export const value = true;\n");
        let error = resolve_js_test_plan(&root.join("packages/app/src/value.ts"), &root)
            .expect_err("malformed workspace metadata must fail closed");
        assert!(
            error.to_string().contains("workspace"),
            "{label}: {error:#}"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
#[test]
fn arbitrary_node_scripts_are_full_suite_for_every_manager() {
    for (label, manager) in [
        ("npm", "npm@10"),
        ("pnpm", "pnpm@9"),
        ("yarn", "yarn@4"),
        ("bun", "bun@1"),
    ] {
        let root = temp_root(label);
        let value = script_plan(&root, manager, "node scripts/run-jest-helper.mjs");
        assert_full_suite(&value, label);
        let _ = std::fs::remove_dir_all(root);
    }
}
#[test]
fn multiple_named_test_scripts_fail_closed_independent_of_order() {
    for (label, scripts) in [
        (
            "first-order",
            r#"{"test:unit":"jest","test:e2e":"playwright test"}"#,
        ),
        (
            "reverse-order",
            r#"{"test:e2e":"playwright test","test:unit":"jest"}"#,
        ),
    ] {
        let root = temp_root(label);
        write_package(
            &root,
            &format!(r#"{{"packageManager":"npm@10","scripts":{scripts}}}"#),
        );
        write(&root, "src/value.ts", "export const value = true;\n");
        let error = resolve_js_test_plan(&root.join("src/value.ts"), &root)
            .expect_err("ambiguous test scripts must fail closed");
        assert!(error.to_string().contains("multiple test:* scripts"));
        let _ = std::fs::remove_dir_all(root);
    }
}
#[test]
fn one_named_test_script_remains_usable() {
    let root = temp_root("single-test-script");
    write_package(
        &root,
        r#"{"packageManager":"npm@10","scripts":{"test:unit":"jest"}}"#,
    );
    write(&root, "src/value.ts", "export const value = true;\n");
    write(&root, "tests/value.test.ts", "test('value', () => {});\n");
    let value = plan(&root);
    assert_eq!(value.command, "npm run test:unit -- tests/value.test.ts");
    assert!(matches!(value.selection, TestSelection::Relevant(_)));
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn child_framework_signal_precedes_workspace_script() {
    for (label, config, framework, command) in [
        ("child-jest", "jest.config.js", TestFramework::Jest, "jest"),
        (
            "child-vitest",
            "vitest.config.ts",
            TestFramework::Vitest,
            "vitest",
        ),
    ] {
        let root = temp_root(label);
        write_workspace_fixture(&root, config);
        let value = resolve_js_test_plan(&root.join("packages/app/src/value.ts"), &root).unwrap();
        assert_eq!(value.framework, Some(framework), "{label}");
        assert!(value.command.contains(command), "{label}");
        assert_eq!(value.working_dir, root.join("packages/app"), "{label}");
        assert!(
            matches!(value.selection, TestSelection::Relevant(_)),
            "{label}"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
#[test]
fn ambiguous_framework_configs_fail_conservatively() {
    let root = temp_root("ambiguous-config");
    write(&root, "package.json", r#"{"packageManager":"npm@10"}"#);
    write(&root, "jest.config.js", "module.exports = {};\n");
    write(&root, "vitest.config.ts", "export default {};\n");
    write(&root, "src/value.ts", "export const value = true;\n");
    write(&root, "tests/value.test.ts", "test('value', () => {});\n");
    let value = plan(&root);
    assert_eq!(value.framework, None);
    assert_eq!(value.selection, TestSelection::FullSuite);
    assert_eq!(value.command, "npm test");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn exact_framework_commands_keep_relevant_selection() {
    for (label, manager, script, framework) in [
        ("npm", "npm@10", "jest", TestFramework::Jest),
        ("pnpm", "pnpm@9", "vitest run", TestFramework::Vitest),
        (
            "yarn",
            "yarn@4",
            "playwright test",
            TestFramework::Playwright,
        ),
        ("bun", "bun@1", "vitest run", TestFramework::Vitest),
    ] {
        let root = temp_root(label);
        let value = script_plan(&root, manager, script);
        assert_eq!(value.framework, Some(framework), "{label}");
        assert!(
            matches!(value.selection, TestSelection::Relevant(_)),
            "{label}"
        );
        assert!(value.command.contains("tests/value.test.ts"), "{label}");
        let _ = std::fs::remove_dir_all(root);
    }
}
#[test]
fn pnpm_workspace_yaml_requires_scalar_package_patterns() {
    assert!(valid_pnpm_workspace_content(
        "packages:\n- packages/*\n- tools/*\n"
    ));
    assert!(valid_pnpm_workspace_content(
        "packages:\n  - packages/*\n  - tools/*\n"
    ));
    assert!(valid_pnpm_workspace_content(
        "sharedWorkspaceLockfile: true\npackages:\n- packages/*\n"
    ));
    for content in [
        "packages:\n- {name: app}\n",
        "packages:\n  - packages/*: app\n",
        "packages:\n  - [packages/*]\n",
        "packages:\n  - \"packages/*\n",
        "packages:\n  - true\n",
        "packages:\n  - packages/*\npackages:\n  - tools/*\n",
        "packages:\n  - packages/*\ntrailing: [\n",
        "packages:\n\t- packages/*\n",
        "packages:\n  \t- packages/*\n",
    ] {
        assert!(!valid_pnpm_workspace_content(content), "{content}");
    }
}
