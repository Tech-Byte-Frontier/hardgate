#[path = "support/fs.rs"]
mod fs;

use hardgate::commands::mutate::select_representative_mutants;
use hardgate::engines::mutation::{
    AstMutant, FULL_SUITE_TIMEOUT_SECS, NativeMutationRunner, PackageManager, TestFramework,
    TestSelection,
};
use std::path::{Path, PathBuf};

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(target, content).unwrap();
}

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        write(root, path, content);
    }
}

fn write_source_pair(root: &Path, source: (&str, &str), test: (&str, &str)) {
    write_files(root, &[source, test]);
}

struct ExpectedPlan {
    manager: PackageManager,
    framework: TestFramework,
    working_dir: PathBuf,
    command_prefix: &'static str,
    test_path: &'static str,
}

fn assert_relevant_plan(
    plan: &hardgate::engines::mutation::ResolvedTestPlan,
    expected: ExpectedPlan,
) {
    assert_eq!(plan.manager, Some(expected.manager));
    assert_eq!(plan.framework, Some(expected.framework));
    assert_eq!(plan.working_dir, expected.working_dir);
    assert!(matches!(plan.selection, TestSelection::Relevant(_)));
    assert!(plan.command.starts_with(expected.command_prefix));
    assert!(plan.command.contains(expected.test_path));
}

fn write_app_fixtures(root: &Path) {
    write_source_pair(
        root,
        (
            "packages/app/src/service.ts",
            "export const ready = true;\n",
        ),
        (
            "packages/app/src/service.test.ts",
            "test('ready', () => {});\n",
        ),
    );
}

fn write_web_fixtures(root: &Path) {
    write_source_pair(
        root,
        ("packages/web/src/page.tsx", "export const page = true;\n"),
        (
            "packages/web/tests/page.spec.tsx",
            "describe('page', () => {});\n",
        ),
    );
}

#[test]
fn resolves_nearest_pnpm_package_and_vitest_file() {
    let root = fs::tempdir("js-nearest-pnpm");
    write_files(
        &root,
        &[
            (
                "package.json",
                r#"{"private":true,"packageManager":"pnpm@9.0.0","workspaces":["packages/*"]}"#,
            ),
            ("pnpm-workspace.yaml", "packages:\n  - packages/*\n"),
            (
                "packages/app/package.json",
                r#"{"name":"app","scripts":{"test":"vitest run"}}"#,
            ),
            ("packages/app/vitest.config.ts", "export default {}\n"),
        ],
    );
    write_app_fixtures(&root);

    let source = root.join("packages/app/src/service.ts");
    let plan = NativeMutationRunner::new(5, None).resolve_test_plan(&source, &root);
    assert_relevant_plan(
        &plan,
        ExpectedPlan {
            manager: PackageManager::Pnpm,
            framework: TestFramework::Vitest,
            working_dir: root.join("packages/app"),
            command_prefix: "pnpm test --",
            test_path: "src/service.test.ts",
        },
    );
    assert_eq!(plan.workspace_root, root);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolves_root_jest_config_for_package_without_local_script() {
    let root = fs::tempdir("js-root-jest");
    write_files(
        &root,
        &[
            (
                "package.json",
                r#"{"private":true,"packageManager":"yarn@4.0.0"}"#,
            ),
            ("yarn.lock", "# lock\n"),
            ("jest.config.cjs", "module.exports = {};\n"),
            ("packages/web/package.json", r#"{"name":"web"}"#),
        ],
    );
    write_web_fixtures(&root);

    let source = root.join("packages/web/src/page.tsx");
    let plan = NativeMutationRunner::new(5, None).resolve_test_plan(&source, &root);
    assert_relevant_plan(
        &plan,
        ExpectedPlan {
            manager: PackageManager::Yarn,
            framework: TestFramework::Jest,
            working_dir: root.clone(),
            command_prefix: "yarn exec jest --",
            test_path: "tests/page.spec.tsx",
        },
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn falls_back_to_full_suite_with_nonzero_timeout() {
    let root = fs::tempdir("js-full-suite");
    write(
        &root,
        "package.json",
        r#"{"packageManager":"bun@1.1.0","scripts":{"test":"node scripts/test.mjs"}}"#,
    );
    write(&root, "bun.lockb", "lock\n");
    write(&root, "src/other.ts", "export const answer = true;\n");
    let source = root.join("src/other.ts");
    let plan = NativeMutationRunner::new(0, None).resolve_test_plan(&source, &root);
    assert_eq!(plan.manager, Some(PackageManager::Bun));
    assert!(plan.framework.is_none());
    assert_eq!(plan.selection, TestSelection::FullSuite);
    assert_eq!(plan.command, "bun test");
    assert!(plan.full_suite_timeout_required());
    assert_eq!(plan.recommended_timeout_secs, FULL_SUITE_TIMEOUT_SECS);
    assert!(NativeMutationRunner::default_timeout_secs(&[source], &root, None) > 0);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn custom_placeholders_remain_unchanged() {
    let root = fs::tempdir("js-custom-command");
    write(&root, "src/widget.ts", "export const widget = true;\n");
    let source = PathBuf::from("src/widget.ts");
    let plan = NativeMutationRunner::new(5, Some("echo {file} {stem}".to_string()))
        .resolve_test_plan(&source, &root);
    assert_eq!(plan.command, "echo src/widget.ts widget");
    assert_eq!(plan.selection, TestSelection::Custom);
    assert_eq!(plan.working_dir, root);
    let _ = std::fs::remove_dir_all(root);
}

fn candidate(file: &str, line: usize, original: &str, replacement: &str) -> AstMutant {
    AstMutant {
        id: 999,
        file: PathBuf::from(file),
        line,
        column: 1,
        start_byte: line,
        end_byte: line + original.len(),
        original: original.to_string(),
        replacement: replacement.to_string(),
        description: format!("{original} -> {replacement}"),
    }
}

#[test]
fn representative_selection_is_sorted_balanced_and_deduplicated() {
    let candidates = vec![
        candidate("z.ts", 4, "==", "!="),
        candidate("a.ts", 3, "==", "!="),
        candidate("a.ts", 1, "&&", "||"),
        candidate("z.ts", 2, "&&", "||"),
        candidate("a.ts", 3, "==", "!="),
        candidate("a.ts", 6, "==", "!="),
    ];
    let selected = select_representative_mutants(candidates, 4);
    assert_eq!(selected.len(), 4);
    assert_eq!(selected[0].file, PathBuf::from("a.ts"));
    assert_eq!(selected[1].file, PathBuf::from("z.ts"));
    assert_eq!(selected[0].line, 1);
    assert_eq!(selected[1].line, 2);
    assert_eq!(selected[0].id, 1);
    assert_eq!(selected[3].id, 4);
}
