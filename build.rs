use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn valid_sha(value: &str) -> Option<String> {
    let trimmed = value.trim();
    ((trimmed.len() == 40 || trimmed.len() == 64)
        && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then(|| trimmed.to_ascii_lowercase())
}

fn cargo_vcs_sha(manifest_dir: &Path) -> Option<String> {
    let path = manifest_dir.join(".cargo_vcs_info.json");
    let text = fs::read_to_string(path).ok()?;
    let marker = "\"sha1\"";
    let start = text.find(marker)? + marker.len();
    let value = text[start..].split(':').nth(1)?.split('"').nth(1)?;
    valid_sha(value)
}

fn git_sha(manifest_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
        .and_then(|value| valid_sha(&value))
}

fn main() {
    println!("cargo:rerun-if-env-changed=HARDGATE_BUILD_GIT_SHA");
    println!("cargo:rerun-if-changed=.cargo_vcs_info.json");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    let manifest_path = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let manifest_dir = Path::new(&manifest_path);
    let sha = env::var("HARDGATE_BUILD_GIT_SHA")
        .ok()
        .and_then(|value| valid_sha(&value))
        .or_else(|| cargo_vcs_sha(manifest_dir))
        .or_else(|| git_sha(manifest_dir))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=HARDGATE_BUILD_GIT_SHA={sha}");
}
