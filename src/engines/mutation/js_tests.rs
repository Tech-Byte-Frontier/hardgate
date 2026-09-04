use super::js::PackageMetadata;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn parse_workspaces(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Array(entries)) => valid_workspace_patterns(entries),
        Some(Value::Object(object)) => object
            .get("packages")
            .and_then(Value::as_array)
            .is_some_and(|entries| valid_workspace_patterns(entries)),
        _ => false,
    }
}

fn valid_workspace_patterns(entries: &[Value]) -> bool {
    !entries.is_empty()
        && entries.iter().all(|entry| {
            entry
                .as_str()
                .is_some_and(|pattern| !pattern.trim().is_empty())
        })
}

pub(crate) fn valid_pnpm_workspace_file(path: &Path) -> bool {
    path.is_file()
        && fs::read_to_string(path)
            .ok()
            .is_some_and(|content| valid_pnpm_workspace_content(&content))
}

fn valid_pnpm_workspace_content(content: &str) -> bool {
    let lines = workspace_lines(content);
    let Some((index, (indent, value))) = lines
        .iter()
        .enumerate()
        .find(|(_, (indent, value))| *indent == 0 && value.starts_with("packages:"))
    else {
        return false;
    };
    let inline = value["packages:".len()..].trim();
    if !inline.is_empty() {
        return parse_inline_workspace_patterns(inline);
    }
    let entries = lines
        .iter()
        .skip(index + 1)
        .take_while(|(next_indent, _)| *next_indent > *indent)
        .collect::<Vec<_>>();
    !entries.is_empty()
        && entries
            .iter()
            .all(|(_, value)| valid_workspace_list_item(value))
}

fn workspace_lines(content: &str) -> Vec<(usize, String)> {
    content.lines().filter_map(parse_workspace_line).collect()
}

fn parse_workspace_line(raw_line: &str) -> Option<(usize, String)> {
    let line = strip_yaml_comment(raw_line);
    let value = line.trim();
    (!value.is_empty()).then(|| (line.len() - line.trim_start().len(), value.to_string()))
}

fn valid_workspace_list_item(value: &str) -> bool {
    value
        .strip_prefix('-')
        .and_then(parse_yaml_string)
        .is_some_and(|item| !item.is_empty())
}

fn parse_inline_workspace_patterns(value: &str) -> bool {
    if let Ok(entries) = serde_json::from_str::<Vec<String>>(value) {
        return !entries.is_empty() && entries.iter().all(|entry| !entry.trim().is_empty());
    }
    let Some(value) = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return false;
    };
    let entries = value.split(',').map(str::trim).collect::<Vec<_>>();
    !entries.is_empty()
        && entries
            .iter()
            .all(|item| parse_yaml_string(item).is_some_and(|entry| !entry.is_empty()))
}

fn parse_yaml_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        return Some(value[1..value.len() - 1].trim().to_string());
    }
    if matches!(
        value.to_ascii_lowercase().as_str(),
        "true" | "false" | "null"
    ) || value.parse::<f64>().is_ok()
    {
        return None;
    }
    (!value.is_empty()).then(|| value.to_string())
}

fn strip_yaml_comment(line: &str) -> String {
    let mut quote = None;
    for (index, character) in line.char_indices() {
        match character {
            '\'' | '"' if quote.is_none() => quote = Some(character),
            character if quote == Some(character) => quote = None,
            '#' if quote.is_none() => return line[..index].to_string(),
            _ => {}
        }
    }
    line.to_string()
}

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
    let mut names = Vec::new();
    for kind in ["test", "spec"] {
        for extension in ordered_extensions(source_extension) {
            names.push(format!("{stem}.{kind}.{extension}"));
        }
    }
    names
}

fn ordered_extensions(source_extension: Option<&str>) -> Vec<&str> {
    let extensions = ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];
    let source_extension = source_extension.map(str::to_ascii_lowercase);
    let mut ordered = Vec::with_capacity(extensions.len());
    if let Some(source) = source_extension.as_deref()
        && let Some(found) = extensions.iter().find(|extension| **extension == source)
    {
        ordered.push(*found);
    }
    ordered.extend(
        extensions
            .iter()
            .filter(|extension| Some(**extension) != source_extension.as_deref())
            .copied(),
    );
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
    if !base.starts_with(package_root) {
        return None;
    }
    names
        .iter()
        .map(|name| base.join(name))
        .find(|path| path.is_file() && path.starts_with(package_root))
}

fn find_nested_test(
    base: &Path,
    names: &[String],
    depth: usize,
    package_root: &Path,
) -> Option<PathBuf> {
    if depth == 0 || !base.is_dir() || !base.starts_with(package_root) {
        return None;
    }
    let mut entries = fs::read_dir(base)
        .ok()?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if is_named_test(&path, names) {
            return Some(path);
        }
        if path.is_dir()
            && path != package_root
            && !is_package_boundary(&path, package_root)
            && !is_pruned_test_dir(&path)
            && let Some(found) = find_nested_test(&path, names, depth - 1, package_root)
        {
            return Some(found);
        }
    }
    None
}

fn is_named_test(path: &Path, names: &[String]) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| names.iter().any(|candidate| candidate == name))
}

fn is_package_boundary(path: &Path, package_root: &Path) -> bool {
    path != package_root && path.join("package.json").is_file()
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
mod tests {
    use super::super::js::{
        PackageManager, ResolvedTestPlan, TestFramework, TestSelection, resolve_js_test_plan,
    };
    use std::path::{Path, PathBuf};

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("hardgate-js-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(root: &Path, path: &str, content: &str) {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, content).unwrap();
    }

    fn plan(root: &Path) -> ResolvedTestPlan {
        resolve_js_test_plan(&root.join("src/value.ts"), root).unwrap()
    }

    fn script_plan(root: &Path, manager: &str, script: &str) -> ResolvedTestPlan {
        write(
            root,
            "package.json",
            &format!(r#"{{"packageManager":"{manager}","scripts":{{"test":"{script}"}}}}"#),
        );
        write(root, "src/value.ts", "export const value = true;\n");
        write(root, "tests/value.test.ts", "test('value', () => {});\n");
        plan(root)
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
        write(&root, "package.json", r#"{"packageManager":"npm@10"}"#);
        write(
            &root,
            "packages/app/package.json",
            r#"{"name":"app","scripts":{"test":"node scripts/test.mjs"}}"#,
        );
        write(&root, "packages/app/pnpm-lock.yaml", "lockfileVersion: 9\n");
        write(
            &root,
            "packages/app/src/value.ts",
            "export const value = true;\n",
        );
        write(
            &root,
            "packages/app/tests/value.test.ts",
            "test('value', () => {});\n",
        );
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
        write(&root, "package.json", r#"{"packageManager":"bun@1"}"#);
        write(&root, "packages/app/package.json", "{\"name\":\"app\",\n");
        write(
            &root,
            "packages/app/src/value.ts",
            "export const value = true;\n",
        );
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
            write(
                &root,
                "package.json",
                &format!(r#"{{"packageManager":"pnpm@9","workspaces":{workspaces}}}"#),
            );
            write(&root, "packages/app/package.json", r#"{"name":"app"}"#);
            write(
                &root,
                "packages/app/src/value.ts",
                "export const value = true;\n",
            );
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
    fn framework_substrings_in_helpers_comments_and_paths_are_not_evidence() {
        for (label, script) in [
            ("helper", "node scripts/vitest-helper.mjs"),
            ("comment", "node scripts/runner.mjs # jest"),
            ("argument", "node scripts/runner.mjs --runner=playwright"),
            ("quoted", "node -e \"// vitest\""),
        ] {
            let root = temp_root(label);
            let value = script_plan(&root, "npm@10", script);
            assert_full_suite(&value, label);
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
}
