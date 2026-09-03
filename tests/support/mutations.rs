//! Mutation fixtures for integration tests.

/// True when a mutant replacing `orig` with `rep` was generated.
pub fn has_mutation(mutants: &[hardgate::engines::AstMutant], orig: &str, rep: &str) -> bool {
    mutants
        .iter()
        .any(|m| m.original == orig && m.replacement == rep)
}
