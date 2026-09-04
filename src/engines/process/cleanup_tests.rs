#[cfg(unix)]
#[test]
fn continuously_writing_kill_helper_is_aborted_with_bounded_capture() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let root = std::env::temp_dir().join(format!("hardgate-kill-shim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let shim = root.join("kill");
    std::fs::write(
        &shim,
        "#!/bin/sh\nwhile :; do printf 'continuous-kill-diagnostic-0123456789\\n' >&2; done\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&shim).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&shim, permissions).unwrap();

    let started = Instant::now();
    let result =
        super::kill::run_bounded_kill_program(shim.to_str().unwrap(), &[], "continuous kill shim");

    assert!(result.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "continuous helper exceeded bounded cleanup"
    );
    let _ = std::fs::remove_dir_all(root);
}
