use super::*;
use std::fs;

#[test]
fn preapply_external_edit_is_preserved_and_reported() {
    let root = std::env::temp_dir().join(format!(
        "hardgate-runner-preapply-edit-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let target = root.join("fixture.rs");
    let marker = root.join("executed");
    fs::write(&target, b"true\n").unwrap();
    let mutant = AstMutant {
        id: 1,
        file: PathBuf::from("fixture.rs"),
        line: 1,
        column: 1,
        start_byte: 0,
        end_byte: 4,
        original: "true".to_string(),
        replacement: "false".to_string(),
        description: "pre-apply edit".to_string(),
    };
    let prepared = prepare_target(&mutant, &root).unwrap();
    fs::write(&target, b"external\n").unwrap();
    let plan = plan::custom_plan(&format!("touch {}", marker.display()), &target, &root);
    let (execution, restored) = execute_and_restore(MutationContext {
        runner: &NativeMutationRunner::new(2, Some(plan.command.clone())),
        mutant: &mutant,
        target_path: &prepared.target_path,
        location: &prepared.location,
        original: &prepared.original,
        plan: &plan,
    });

    assert_eq!(execution.outcome, MutantOutcome::RunnerError);
    assert!(!restored);
    assert!(
        execution
            .diagnostic
            .contains("changed after its initial snapshot")
    );
    assert_eq!(fs::read(&target).unwrap(), b"external\n");
    assert!(!marker.exists());
    let _ = fs::remove_dir_all(root);
}
