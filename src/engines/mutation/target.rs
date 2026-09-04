use crate::config::HardgateConfig;
use crate::discovery::{ClassifiedFile, FileRole};

pub(crate) fn is_effective_mutation_target(
    classified: &ClassifiedFile,
    config: &HardgateConfig,
) -> bool {
    let builtin_role = ClassifiedFile::new(&classified.path).role;
    if !matches!(builtin_role, FileRole::Source | FileRole::Unknown) {
        return false;
    }
    classified.role == FileRole::Source
        && config
            .roles
            .source
            .mutation_target
            .unwrap_or_else(|| FileRole::Source.is_mutation_target())
}
