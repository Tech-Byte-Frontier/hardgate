use super::super::generator::AstMutant;
use super::super::js::ResolvedTestPlan;
use super::super::process::{CommandExecution, execute_with_timeout};
use super::plan::process_roots;
use super::restore::{
    AtomicReplacement, ExpectedEntry, RestoreLocation, SourceSnapshot, atomic_replace_location,
    same_permissions, same_snapshot_identity, snapshot_location,
};
use super::{MutantOutcome, NativeMutationRunner};

pub(super) struct MutationInput<'a> {
    pub(super) mutant: &'a AstMutant,
    pub(super) location: &'a RestoreLocation,
    pub(super) original: &'a SourceSnapshot,
    pub(super) plan: &'a ResolvedTestPlan,
}

pub(super) fn apply_and_execute(
    runner: &NativeMutationRunner,
    input: MutationInput<'_>,
    expected_mutation: &mut Option<SourceSnapshot>,
) -> CommandExecution {
    match apply_mutant_bytes(
        input.location,
        input.mutant,
        input.original,
        expected_mutation,
    ) {
        Ok(ApplyResult::Equivalent) => equivalent_execution(),
        Ok(ApplyResult::Applied) => execute_applied(runner, input, expected_mutation),
        Err(error) => apply_error(input.mutant, error),
    }
}

fn equivalent_execution() -> CommandExecution {
    CommandExecution {
        outcome: MutantOutcome::Equivalent,
        diagnostic: "Replacement is byte-for-byte equivalent to the original source text."
            .to_string(),
        status: None,
    }
}

fn execute_applied(
    runner: &NativeMutationRunner,
    input: MutationInput<'_>,
    expected_mutation: &mut Option<SourceSnapshot>,
) -> CommandExecution {
    match snapshot_location(input.location) {
        Ok(Some(snapshot)) => {
            let Some(expected) = expected_mutation.as_ref() else {
                return CommandExecution {
                    outcome: MutantOutcome::RunnerError,
                    diagnostic: "Mutation replacement was not armed before execution".to_string(),
                    status: None,
                };
            };
            if !replacement_matches(&snapshot, expected) {
                return CommandExecution {
                    outcome: MutantOutcome::RunnerError,
                    diagnostic: "Mutation target changed immediately after atomic application"
                        .to_string(),
                    status: None,
                };
            }
            execute_with_timeout(
                &input.plan.command,
                process_roots(input.plan),
                runner.timeout_secs,
            )
        }
        Ok(None) => CommandExecution {
            outcome: MutantOutcome::RunnerError,
            diagnostic: "Mutation target disappeared after atomic application".to_string(),
            status: None,
        },
        Err(error) => CommandExecution {
            outcome: MutantOutcome::RunnerError,
            diagnostic: format!(
                "Failed to verify mutation target after atomic application: {error}"
            ),
            status: None,
        },
    }
}

fn replacement_matches(current: &SourceSnapshot, expected: &SourceSnapshot) -> bool {
    same_snapshot_identity(current, expected)
        && current.bytes == expected.bytes
        && same_permissions(&current.permissions, &expected.permissions)
}

fn apply_error(mutant: &AstMutant, error: std::io::Error) -> CommandExecution {
    let outcome = if error.kind() == std::io::ErrorKind::InvalidInput {
        MutantOutcome::Unviable
    } else {
        MutantOutcome::RunnerError
    };
    CommandExecution {
        outcome,
        diagnostic: format!("Failed to apply mutant {}: {error}", mutant.id),
        status: None,
    }
}

fn apply_mutant_bytes(
    location: &RestoreLocation,
    mutant: &AstMutant,
    original: &SourceSnapshot,
    armed: &mut Option<SourceSnapshot>,
) -> std::io::Result<ApplyResult> {
    let original_bytes = &original.bytes;
    if mutant.start_byte > mutant.end_byte || mutant.end_byte > original_bytes.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "mutant byte range out of bounds",
        ));
    }
    if &original_bytes[mutant.start_byte..mutant.end_byte] != mutant.original.as_bytes() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "mutant original text does not match the source bytes",
        ));
    }
    if mutant.replacement.as_bytes() == &original_bytes[mutant.start_byte..mutant.end_byte] {
        return Ok(ApplyResult::Equivalent);
    }
    let mut mutated = Vec::with_capacity(
        original_bytes.len() - (mutant.end_byte - mutant.start_byte) + mutant.replacement.len(),
    );
    mutated.extend_from_slice(&original_bytes[..mutant.start_byte]);
    mutated.extend_from_slice(mutant.replacement.as_bytes());
    mutated.extend_from_slice(&original_bytes[mutant.end_byte..]);
    let current = snapshot_location(location)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "mutation target disappeared after its initial snapshot",
        )
    })?;
    if !same_snapshot_identity(&current, original)
        || current.bytes != original.bytes
        || !same_permissions(&current.permissions, &original.permissions)
    {
        return Err(std::io::Error::other(
            "mutation target changed after its initial snapshot; refusing to apply",
        ));
    }
    atomic_replace_location(
        location,
        AtomicReplacement {
            bytes: &mutated,
            permissions: &original.permissions,
            expected: ExpectedEntry::Present(original),
            armed: Some(armed),
        },
    )
    .map(|()| ApplyResult::Applied)
}

#[derive(Clone, Copy)]
enum ApplyResult {
    Applied,
    Equivalent,
}
