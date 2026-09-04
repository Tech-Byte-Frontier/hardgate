pub mod anti_gaming;
pub mod budgets;
pub mod clones;
pub mod complexity;
pub mod coverage;
pub mod dead_code;
pub mod invariants;
pub mod mutation;
pub mod orchestration;
pub mod util;

pub use anti_gaming::{AntiGamingScanner, SuppressionViolation};
pub use budgets::{BudgetViolation, check_file_budgets};
pub use clones::{CloneDetector, CloneViolation};
pub use complexity::{
    ComplexityAnalyzer, ComplexityContribution, ComplexityViolation, FunctionMetrics,
};
pub use coverage::{CoverageScorer, CoverageViolation};
pub use dead_code::{DeadCodeAnalyzer, DeadCodeViolation};
pub use invariants::{InvariantViolation, InvariantsChecker};
pub use mutation::{
    AstMutant, AstMutationGenerator, BaselineExecutionResult, BaselineOutcome,
    MutantExecutionResult, MutantOutcome, MutationGatekeeper, MutationStats, MutationViolation,
    NativeMutationRunner,
};
pub use orchestration::{OrchestrationEngine, OrchestrationResult, OrchestrationViolation};
