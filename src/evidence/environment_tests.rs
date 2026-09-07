use super::*;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{PermissionsExt, symlink};

#[test]
fn interpreter_exceptions_require_a_recognized_environment_and_config() {
    let root = crate::fs_tests::tempdir("interpreter-recognition");
    for relative in [
        "python",
        "tools/python",
        "tools/bin/python",
        "bin/python",
        ".venv/bin/tool",
        ".venv/bin/python-dev",
        "venv/bin/python3",
        ".tox/unit/bin/python",
        ".venv/bin/python",
    ] {
        assert_eq!(
            interpreter_target(&root, Path::new(relative)).unwrap(),
            None,
            "{relative}"
        );
    }
    let invalid = PathBuf::from(".venv/bin").join(std::ffi::OsString::from_vec(vec![0xff]));
    assert_eq!(interpreter_target(&root, &invalid).unwrap(), None);
    fs::create_dir(root.join(".venv")).unwrap();
    let marker = root.join(".venv/pyvenv.cfg");
    fs::write(
        &marker,
        "ignored line\ninclude-system-site-packages = false\n",
    )
    .unwrap();
    assert_eq!(
        interpreter_target(&root, Path::new(".venv/bin/python")).unwrap(),
        None
    );
    fs::remove_file(&marker).unwrap();
    fs::create_dir(&marker).unwrap();
    assert!(interpreter_target(&root, Path::new(".venv/bin/python")).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn declared_interpreter_aliases_are_verified_against_the_real_executable() {
    let root = crate::fs_tests::tempdir("interpreter-aliases")
        .canonicalize()
        .unwrap();
    fs::create_dir_all(root.join(".venv/bin")).unwrap();
    fs::create_dir(root.join("runtime")).unwrap();
    let runtime = root.join("runtime/python3");
    fs::write(&runtime, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let marker = root.join(".venv/pyvenv.cfg");
    fs::write(
        &marker,
        format!("home = {}\n", root.join("runtime").display()),
    )
    .unwrap();
    for name in ["python", "python3", "python3.13"] {
        let relative = PathBuf::from(".venv/bin").join(name);
        symlink(&runtime, root.join(&relative)).unwrap();
        assert_eq!(
            interpreter_target(&root, &relative).unwrap(),
            Some(runtime.clone())
        );
    }
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(interpreter_target(&root, Path::new(".venv/bin/python")).is_err());
    fs::write(&marker, "home = relative\n").unwrap();
    assert!(interpreter_target(&root, Path::new(".venv/bin/python")).is_err());
    fs::remove_dir_all(root).unwrap();
}
