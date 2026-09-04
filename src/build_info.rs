/// Build identity embedded by `build.rs` in every binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_SHA: &str = env!("HARDGATE_BUILD_GIT_SHA");
pub const TARGET: &str = env!("HARDGATE_BUILD_TARGET");
pub const VERSION_DISPLAY: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("HARDGATE_BUILD_GIT_SHA"),
    ")"
);

// Kept as a used static so release verification can positively bind an ELF
// payload to Cargo's exact target triple even when release symbols are stripped.
#[used]
pub static BUILD_TARGET_MARKER: &str = concat!("hardgate-target:", env!("HARDGATE_BUILD_TARGET"));

pub const fn identity() -> (&'static str, &'static str) {
    (VERSION, GIT_SHA)
}
