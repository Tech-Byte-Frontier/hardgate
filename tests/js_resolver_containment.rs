#[path = "support/fs.rs"]
mod fs;

use hardgate::engines::NativeMutationRunner;
use std::path::Path;

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(target, content).unwrap();
}

#[test]
fn resolver_rejects_absolute_and_symlink_source_escape() {
    let root = fs::tempdir("js-source-containment-root");
    write(&root, "package.json", r#"{"packageManager":"npm@10"}"#);
    let outside = fs::tempdir("js-source-containment-outside");
    write(&outside, "package.json", "{\n");
    write(&outside, "src/value.ts", "export const value = true;\n");
    let runner = NativeMutationRunner::new(5, None);
    let check = |source: &Path| {
        let error = runner
            .resolve_test_plan(source, &root)
            .expect_err("source escaping repository root must fail closed");
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
