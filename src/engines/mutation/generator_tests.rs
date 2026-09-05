use super::{AstMutant, AstMutationGenerator};
use std::path::{Path, PathBuf};

type MutantSignature = (
    usize,
    PathBuf,
    usize,
    usize,
    usize,
    usize,
    String,
    String,
    String,
);

fn signatures(mutants: &[AstMutant]) -> Vec<MutantSignature> {
    mutants
        .iter()
        .map(|mutant| {
            (
                mutant.id,
                mutant.file.clone(),
                mutant.line,
                mutant.column,
                mutant.start_byte,
                mutant.end_byte,
                mutant.original.clone(),
                mutant.replacement.clone(),
                mutant.description.clone(),
            )
        })
        .collect()
}

fn source() -> &'static str {
    "fn evaluate(a: i32, b: i32) -> bool { if a == b && a > 0 { true } else { false } }"
}

#[test]
fn bounded_generation_matches_unlimited_generation_when_capacity_is_sufficient() {
    let path = Path::new("src/generator_fixture.rs");
    let mut unlimited = AstMutationGenerator::new();
    let expected = unlimited.generate_mutants(path, source());
    assert!(!expected.is_empty());

    let mut bounded = AstMutationGenerator::new();
    let actual = bounded
        .generate_mutants_bounded(path, source(), expected.len())
        .expect("sufficient candidate limit should succeed");

    assert_eq!(signatures(&actual), signatures(&expected));
}

#[test]
fn bounded_generation_returns_resource_error_without_partial_success() {
    let path = Path::new("src/generator_fixture.rs");
    let mut generator = AstMutationGenerator::new();
    let content = source().to_owned();
    let original = content.clone();

    let error = generator
        .generate_mutants_bounded(path, &content, 1)
        .expect_err("insufficient candidate limit should fail");

    assert!(
        error
            .to_string()
            .starts_with("mutation resource guard: mutant candidate limit exceeded:"),
        "{error}"
    );
    assert_eq!(content, original);
}

#[test]
fn bounded_generation_preserves_source_content_on_zero_limit() {
    let path = Path::new("src/generator_fixture.rs");
    let content = "fn unchanged() -> bool { true }";
    let mut generator = AstMutationGenerator::new();

    assert!(
        generator
            .generate_mutants_bounded(path, content, 0)
            .is_err()
    );
    assert_eq!(content, "fn unchanged() -> bool { true }");
}

#[test]
fn deep_tree_generation_uses_cursor_without_recursive_call_stack_growth() {
    let depth = 4096;
    let mut content = String::with_capacity(depth * 2 + 32);
    content.push_str("fn deep() -> bool { ");
    for _ in 0..depth {
        content.push('(');
    }
    content.push_str("true");
    for _ in 0..depth {
        content.push(')');
    }
    content.push_str(" }");

    let mut generator = AstMutationGenerator::new();
    let mutants = generator.generate_mutants(Path::new("src/deep.rs"), &content);
    let true_start = content
        .find("true")
        .expect("deep fixture should contain true");
    let true_end = true_start + "true".len();

    assert_eq!(mutants.len(), 2);
    assert!(mutants.iter().all(|mutant| {
        mutant.original == "true"
            && mutant.replacement == "false"
            && mutant.start_byte == true_start
            && mutant.end_byte == true_end
    }));
}
