/// Build identity embedded by `build.rs` in every binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_SHA: &str = env!("HARDGATE_BUILD_GIT_SHA");
pub const VERSION_DISPLAY: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("HARDGATE_BUILD_GIT_SHA"),
    ")"
);

pub const fn identity() -> (&'static str, &'static str) {
    (VERSION, GIT_SHA)
}
