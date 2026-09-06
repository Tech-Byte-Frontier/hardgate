use super::{SourceSnapshot, StaticRequest, analyze_snapshot, classify_files};
use crate::config::Preset;
use std::fs;
use std::sync::Arc;

use crate::fs_tests as support;

#[test]
fn all_engines_keep_captured_bytes_after_the_worktree_changes() {
    let root = support::tempdir("shared-source-snapshot");
    let exported = "export function chosen() { return 17; }\n";
    let inputs = [
        ("a.ts", exported),
        ("b.ts", exported),
        (
            "index.ts",
            "import { chosen } from './a';\nimport { chosen as second } from './b';\nchosen(); second();\n",
        ),
    ];
    let files = inputs
        .iter()
        .map(|(name, text)| {
            let path = root.join(name);
            fs::write(&path, text).unwrap();
            path
        })
        .collect::<Vec<_>>();
    let mut config = Preset::Custom.to_default_config();
    config.clones.min_lines = 1;
    config.clones.min_tokens = 8;
    let snapshot = SourceSnapshot::capture(classify_files(&files, &config, &root).unwrap());
    let captured = snapshot.shared_contents(&files);
    fs::write(&files[0], [0xff]).unwrap();
    fs::write(&files[1], "").unwrap();
    fs::write(&files[2], "// no remaining references\n").unwrap();

    let outcome = analyze_snapshot(
        StaticRequest {
            config: &config,
            root: &root,
            paths: &[],
            diff: true,
            snippets: false,
        },
        files.clone(),
        Vec::new(),
        snapshot,
    )
    .unwrap();
    assert_eq!(outcome.functions.len(), 2);
    assert_eq!(outcome.report.clone_violations.len(), 1);
    assert!(outcome.report.orchestration_violations.is_empty());
    for ((path, text), (expected_path, expected_text)) in outcome.read_results.iter().zip(&captured)
    {
        assert_eq!(path, expected_path);
        assert!(Arc::ptr_eq(text, expected_text));
    }
    assert_eq!(fs::read(&files[0]).unwrap(), [0xff]);
    fs::remove_dir_all(root).unwrap();
}
