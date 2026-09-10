use super::*;

pub(super) fn publish(produced: ProductionOutput, publication: Publication<'_>) -> CommandResult {
    let ProductionOutput {
        producer,
        spec,
        version,
        exit,
        partition,
        runtime_inputs,
    } = produced;
    let Publication {
        root,
        destination,
        before,
        workspace,
        config,
        input_policy,
    } = publication;
    workspace.begin_publication()?;
    if let Some(identity) = &runtime_inputs {
        identity.require_same(&runtime_inputs::RuntimeInputs::capture(root, producer)?)?;
    }
    let bytes = normalized_report(&spec.report, workspace.root(), producer)?;
    let temporary = destination.with_extension("pending");
    fs::write(&temporary, &bytes)?;
    let validation = validate_producer_report(
        producer,
        &temporary,
        &EvidenceInputs {
            snapshot: &before,
            config,
            root,
        },
    );
    if let Err(error) = validation {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Some(partition) = &partition {
        aggregation::validate_partition_report(producer, &temporary, (root, config), partition)?;
    }
    let prerequisite_passed = spec.prerequisite.is_some();
    let receipt = Receipt {
        schema_version: 2,
        root: root.to_path_buf(),
        producer,
        producer_version: version,
        command: spec
            .prerequisite
            .into_iter()
            .chain(spec.auxiliary)
            .chain(std::iter::once(spec.tokens))
            .collect(),
        runner_exit: exit,
        inputs: before,
        report_sha256: file_hash(&temporary)?,
        restoration_verified: true,
        prerequisite_passed,
        workspace: Some(workspace.job_path().to_path_buf()),
        partition,
        runtime_inputs,
    };
    save_verified(
        receipt,
        workspace,
        &temporary,
        PublicationDestination {
            root,
            destination,
            input_policy,
        },
    )
}

struct PublicationDestination<'a> {
    root: &'a Path,
    destination: &'a Path,
    input_policy: inputs::InputPolicy,
}

fn save_verified(
    receipt: Receipt,
    workspace: workspace::EvidenceWorkspace,
    temporary: &Path,
    target: PublicationDestination<'_>,
) -> CommandResult {
    let PublicationDestination {
        root,
        destination,
        input_policy,
    } = target;
    receipt.inputs.require_same(
        &Snapshot::capture_with(root, &input_policy)?,
        "checkout before evidence publication",
    )?;
    crate::cancellation::check()?;
    fs::rename(temporary, destination)?;
    let mut publication = PendingReceipt {
        path: receipt_path(destination),
        committed: false,
    };
    let receipt_text = serde_json::to_string_pretty(&receipt)?;
    crate::commands::outcome::write_atomic_file(&publication.path, &receipt_text)?;
    ensure!(
        file_hash(destination)? == receipt.report_sha256,
        "published report bytes differ from verified producer output"
    );
    ensure!(
        fs::read_to_string(&publication.path)? == receipt_text,
        "published receipt bytes differ from verified execution"
    );
    receipt.inputs.require_same(
        &Snapshot::capture_with(root, &input_policy)?,
        "checkout before publication completion",
    )?;
    // Both artifacts are saved and verified outside the copy. A reader rejects
    // this receipt while the active job exists; cleanup failure revokes it.
    let certification =
        authentication::certify(receipt_text.as_bytes(), root, workspace.root(), destination)?;
    workspace.close()?;
    crate::cancellation::check()?;
    certification.commit();
    publication.committed = true;
    crate::engines::process::diagnostic(format_args!(
        "source-bound evidence: {}",
        destination.display()
    ));
    Ok(if receipt.runner_exit == 0 {
        CommandOutcome::Passed
    } else {
        CommandOutcome::Violations
    })
}

struct PendingReceipt {
    path: PathBuf,
    committed: bool,
}
impl Drop for PendingReceipt {
    fn drop(&mut self) {
        if !self.committed
            && let Err(error) = fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            crate::engines::process::diagnostic(format_args!(
                "hardgate: failed to revoke incomplete receipt {}: {error}",
                self.path.display()
            ));
        }
    }
}
