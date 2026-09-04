use super::run_mutant_batch;
use crate::engines::{AstMutant, NativeMutationRunner};
use std::path::{Path, PathBuf};

fn mutant(id: usize) -> AstMutant {
    AstMutant {
        id,
        file: PathBuf::from("nested/fixture.rs"),
        line: 1,
        column: 1,
        start_byte: 0,
        end_byte: 4,
        original: "true".to_string(),
        replacement: "false".to_string(),
        description: "batch safety mutant".to_string(),
    }
}

#[cfg(unix)]
#[test]
fn batch_aborts_after_restore_failure_before_starting_later_mutants() {
    let root = std::env::temp_dir().join(format!("hardgate-batch-restore-{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!(
        "hardgate-batch-restore-outside-{}",
        std::process::id()
    ));
    let nested = root.join("nested");
    let outside_target = outside.join("fixture.rs");
    let second_marker = root.join("second-mutant-started");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(nested.join("fixture.rs"), b"true\n").unwrap();
    std::fs::write(&outside_target, b"outside\n").unwrap();
    let command = format!(
        "sh -c 'if [ ! -e batch-first ]; then touch batch-first; rm -rf nested; ln -s {} nested; else touch {}; fi'",
        outside.display(),
        second_marker.display()
    );
    let runner = NativeMutationRunner::new(2, Some(command));
    let mutants = [mutant(1), mutant(2)];

    let result = run_mutant_batch(&mutants, &runner, Path::new(&root));

    assert!(result.is_err());
    assert!(!second_marker.exists());
    assert_eq!(std::fs::read(&outside_target).unwrap(), b"outside\n");
    assert!(
        std::fs::symlink_metadata(&nested)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}
