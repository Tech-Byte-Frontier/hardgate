use crate::config::HardgateConfig;
use crate::discovery::ClassifiedFile;

pub(crate) fn is_effective_mutation_target(
    classified: &ClassifiedFile,
    config: &HardgateConfig,
) -> bool {
    config
        .roles
        .for_role(classified.role)
        .and_then(|policy| policy.mutation_target)
        .unwrap_or_else(|| classified.role.is_mutation_target())
}
