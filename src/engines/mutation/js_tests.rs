use super::js_manifest::PackageMetadata;
use std::fs;
use std::path::{Path, PathBuf};
pub(crate) fn find_relevant_test(
    source: &Path,
    execution_root: &Path,
    package: Option<&PackageMetadata>,
) -> Option<PathBuf> {
    let stem = source.file_stem()?.to_str()?;
    let extension = source.extension().and_then(|value| value.to_str());
    let names = test_names(stem, extension);
    let package_root = package
        .map(|item| item.root.as_path())
        .unwrap_or(execution_root);
    let bases = direct_test_bases(source, execution_root, package_root);
    for base in deduplicate_paths(bases) {
        if let Some(found) = find_direct_test(&base, &names, package_root) {
            return Some(found);
        }
    }
    for base in deduplicate_paths(test_roots(execution_root, package_root)) {
        if let Some(found) = find_nested_test(&base, &names, 4, package_root) {
            return Some(found);
        }
    }
    None
}
fn direct_test_bases(source: &Path, execution_root: &Path, package_root: &Path) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(parent) = source.parent()
        && parent.starts_with(package_root)
    {
        bases.push(parent.to_path_buf());
        bases.push(parent.join("__tests__"));
    }
    bases.push(package_root.join("tests"));
    bases.push(package_root.join("__tests__"));
    if let Some(parent) = source
        .parent()
        .and_then(|path| path.strip_prefix(package_root).ok())
    {
        bases.push(package_root.join("tests").join(parent));
        bases.push(package_root.join("__tests__").join(parent));
    }
    if execution_root != package_root && execution_root.starts_with(package_root) {
        bases.push(execution_root.join("tests"));
        bases.push(execution_root.join("__tests__"));
    }
    bases
}
fn test_names(stem: &str, source_extension: Option<&str>) -> Vec<String> {
    ["test", "spec"]
        .into_iter()
        .flat_map(|kind| {
            ordered_extensions(source_extension)
                .into_iter()
                .map(move |extension| format!("{stem}.{kind}.{extension}"))
        })
        .collect()
}
fn ordered_extensions(source_extension: Option<&str>) -> Vec<&str> {
    let extensions = ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];
    let source_extension = source_extension.map(str::to_ascii_lowercase);
    let mut ordered = extensions
        .iter()
        .filter(|extension| Some(**extension) != source_extension.as_deref())
        .copied()
        .collect::<Vec<_>>();
    if let Some(source) = source_extension
        .as_deref()
        .filter(|source| extensions.contains(source))
    {
        ordered.insert(0, source);
    }
    ordered
}
fn test_roots(execution_root: &Path, package_root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![package_root.join("tests"), package_root.join("__tests__")];
    if execution_root != package_root && execution_root.starts_with(package_root) {
        roots.push(execution_root.join("tests"));
        roots.push(execution_root.join("__tests__"));
    }
    roots
}
fn find_direct_test(base: &Path, names: &[String], package_root: &Path) -> Option<PathBuf> {
    if !within_package_root(base, package_root) || is_nested_package(base, package_root) {
        return None;
    }
    names
        .iter()
        .map(|name| base.join(name))
        .find(|path| path.is_file() && within_package_root(path, package_root))
}
fn find_nested_test(
    base: &Path,
    names: &[String],
    depth: usize,
    package_root: &Path,
) -> Option<PathBuf> {
    if !can_search_nested(base, depth, package_root) {
        return None;
    }
    let mut entries = fs::read_dir(base)
        .ok()?
        .filter_map(std::result::Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    entries
        .into_iter()
        .find_map(|entry| find_nested_entry(&entry.path(), names, depth, package_root))
}
fn can_search_nested(base: &Path, depth: usize, package_root: &Path) -> bool {
    depth > 0
        && base.is_dir()
        && within_package_root(base, package_root)
        && !is_nested_package(base, package_root)
}
fn find_nested_entry(
    path: &Path,
    names: &[String],
    depth: usize,
    package_root: &Path,
) -> Option<PathBuf> {
    if is_named_test(path, names, package_root) {
        return Some(path.to_path_buf());
    }
    if !path.is_dir()
        || path == package_root
        || is_nested_package(path, package_root)
        || is_pruned_test_dir(path)
        || !within_package_root(path, package_root)
    {
        return None;
    }
    find_nested_test(path, names, depth.saturating_sub(1), package_root)
}
fn is_named_test(path: &Path, names: &[String], package_root: &Path) -> bool {
    path.is_file()
        && within_package_root(path, package_root)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| names.iter().any(|candidate| candidate == name))
}
fn is_nested_package(path: &Path, package_root: &Path) -> bool {
    path != package_root && fs::symlink_metadata(path.join("package.json")).is_ok()
}
fn within_package_root(path: &Path, package_root: &Path) -> bool {
    let canonical_root =
        fs::canonicalize(package_root).unwrap_or_else(|_| package_root.to_path_buf());
    let canonical_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical_path.starts_with(canonical_root)
}
fn is_pruned_test_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "node_modules" | "dist" | "build" | "vendor"))
}
fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut output = Vec::new();
    for path in paths {
        if !output.contains(&path) {
            output.push(path);
        }
    }
    output
}
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_TEMP_ROOT: AtomicUsize = AtomicUsize::new(0);
    const WORKSPACE_PACKAGE: &str =
        r#"{"workspaces":["packages/*"],"scripts":{"test":"node root.mjs"}}"#;
    pub(crate) fn temp_root(label: &str) -> PathBuf {
        let id = NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("hardgate-js-{label}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    pub(crate) fn write(root: &Path, path: &str, content: &str) {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, content).unwrap();
    }
    pub(crate) fn write_workspace_fixture(root: &Path, config: &str) {
        write(root, "package.json", WORKSPACE_PACKAGE);
        write(root, "packages/app/package.json", r#"{"name":"app"}"#);
        write(root, &format!("packages/app/{config}"), "");
        for (path, content) in [
            ("packages/app/src/value.ts", "export const value = true;\n"),
            (
                "packages/app/tests/value.test.ts",
                "test('value', () => {});\n",
            ),
        ] {
            write(root, path, content);
        }
    }
}
#[cfg(test)]
mod tests {
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
    fn invalid_package_workspaces_shapes_do_not_create_boundaries() {
        for (label, workspaces) in [
            ("boolean", "true"),
            ("null", "null"),
            ("empty-array", "[]"),
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
            let value =
                resolve_js_test_plan(&root.join("packages/app/src/value.ts"), &root).unwrap();
            assert_eq!(value.workspace_root, root.join("packages/app"), "{label}");
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
            let value =
                resolve_js_test_plan(&root.join("packages/app/src/value.ts"), &root).unwrap();
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
}
