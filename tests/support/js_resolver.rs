use std::fmt::Display;
use std::path::{Path, PathBuf};

pub fn assert_source_escape_rejected<M, R, E>(temp_root: M, resolve: R)
where
    M: Fn(&str) -> PathBuf,
    R: Fn(&Path, &Path) -> Result<(), E>,
    E: Display,
{
    let root = temp_root("source-root");
    write(&root, "package.json", r#"{"packageManager":"npm@10"}"#);
    let outside = temp_root("source-outside");
    write(&outside, "package.json", "{\n");
    write(&outside, "src/value.ts", "export const value = true;\n");
    let check = |source: &Path| {
        let error =
            resolve(source, &root).expect_err("source escaping repository root must fail closed");
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

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(target, content).unwrap();
}
