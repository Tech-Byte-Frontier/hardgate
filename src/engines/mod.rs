pub mod anti_gaming;
pub mod budgets;
pub mod clones;
pub mod complexity;
pub mod coverage;
pub mod generated;
pub mod invariants;
pub mod mutation;
pub mod orchestration;
pub(crate) mod process;
pub use process::configure_progress_jsonl;
pub mod util;

pub use anti_gaming::{AntiGamingScanner, SuppressionViolation};
pub use budgets::{BudgetViolation, check_content_budgets, check_file_budgets};
pub use clones::{CloneDetector, CloneViolation};
pub use complexity::{
    ComplexityAnalyzer, ComplexityContribution, ComplexityViolation, FunctionMetrics,
};
pub use coverage::{CoverageScorer, CoverageViolation};
pub use generated::run_generated_freshness;
pub use invariants::{InvariantViolation, InvariantsChecker};
pub use mutation::{MutationGatekeeper, MutationStats, MutationViolation};
pub use orchestration::{OrchestrationEngine, OrchestrationResult, OrchestrationViolation};
pub mod cargo_diagnostics;
