#![cfg(any(target_os = "linux", target_os = "macos"))]

use hardgate::engines::{AstMutant, MutantOutcome, NativeMutationRunner};
use std::path::{Path, PathBuf};

#[test]
fn no_write_mutant_paths_preserve_inode_content_and_mode() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let root = std::env::temp_dir().join(format!(
        "hardgate-mutation-no-write-identity-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("fixture.rs");
    let marker = root.join("executed");
    std::fs::write(&target, b"true\n").unwrap();
    let identity = |path: &Path| {
        let metadata = std::fs::metadata(path).unwrap();
        (
            metadata.dev(),
            metadata.ino(),
            metadata.nlink(),
            metadata.permissions().mode(),
        )
    };
    let before = identity(&target);
    let runner = NativeMutationRunner::new(2, Some(format!("touch {}", marker.display())));
    let equivalent = AstMutant {
        id: 1,
        file: PathBuf::from("fixture.rs"),
        line: 1,
        column: 1,
        start_byte: 0,
        end_byte: 4,
        original: "true".to_string(),
        replacement: "true".to_string(),
        description: "equivalent".to_string(),
    };
    let equivalent_result = runner.run_mutant(&equivalent, Path::new(&root));
    assert_eq!(equivalent_result.outcome, MutantOutcome::Equivalent);
    assert!(equivalent_result.source_restored);
    assert_eq!(identity(&target), before);
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    assert!(!marker.exists());

    let unviable = AstMutant {
        start_byte: 99,
        end_byte: 100,
        ..equivalent
    };
    let unviable_result = runner.run_mutant(&unviable, Path::new(&root));
    assert_eq!(unviable_result.outcome, MutantOutcome::Unviable);
    assert!(unviable_result.source_restored);
    assert_eq!(identity(&target), before);
    assert_eq!(std::fs::read(&target).unwrap(), b"true\n");
    let _ = std::fs::remove_dir_all(root);
}
