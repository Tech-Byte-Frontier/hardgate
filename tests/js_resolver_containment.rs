#[path = "support/fs.rs"]
mod fs;
#[path = "support/js_resolver.rs"]
mod js_resolver_support;

use hardgate::engines::NativeMutationRunner;

#[test]
fn resolver_rejects_absolute_and_symlink_source_escape() {
    let runner = NativeMutationRunner::new(5, None);
    js_resolver_support::assert_source_escape_rejected(fs::tempdir, |source, root| {
        runner
            .resolve_test_plan(source, root)
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
}
