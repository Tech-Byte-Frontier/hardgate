use super::super::test_support::temp_root as shared_temp_root;
use std::path::{Path, PathBuf};
const WORKSPACE_PACKAGE: &str =
    r#"{"workspaces":["packages/*"],"scripts":{"test":"node root.mjs"}}"#;

pub(crate) fn temp_root(label: &str) -> PathBuf {
    shared_temp_root("hardgate-js", label)
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
