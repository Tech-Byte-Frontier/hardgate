use super::*;

/// Produce fresh evidence from an independent input copy. This operation never
/// attaches a receipt to a pre-existing report supplied by the caller.
pub fn produce(mut options: EvidenceOptions, context: &ConfigContext) -> CommandResult {
    let _phase = crate::engines::process::phase::set(&format!(
        "evidence:{}",
        options
            .producer_config
            .as_deref()
            .unwrap_or(options.producer.name())
    ));
    crate::resources::runtime::require()?;
    ensure!(
        options.timeout_secs > 0,
        "evidence timeout must be positive"
    );
    let root = context.root.canonicalize()?;
    let partition = partitions::resolve(&mut options, context)?;
    let destination = output_path(&root, &options)?;
    authentication::revoke(&destination, &root)?;
    remove_receipt(&destination)?;
    let input_policy = inputs::InputPolicy::new(&root, &context.config)?;
    let before = Snapshot::capture_with(&root, &input_policy)?;
    ensure!(
        !before.0.is_empty(),
        "evidence requires non-empty project inputs"
    );
    let workspace = workspace::EvidenceWorkspace::create_verified(&root, &input_policy, &before)?;
    let job = workspace.job_path().to_path_buf();
    let runtime_inputs =
        if partition.is_some() && runtime_inputs::can_reuse(&root, options.producer) {
            Some(runtime_inputs::RuntimeInputs::capture(
                &root,
                options.producer,
            )?)
        } else {
            None
        };
    let result = run_producer(
        &options,
        partition,
        runtime_inputs,
        Publication {
            root: &root,
            destination: &destination,
            before,
            workspace,
            config: &context.config,
            input_policy,
        },
    );
    result.with_context(|| {
        format!(
            "evidence run job={} workspace={} retained={}; inspect lifecycle.json and diagnostics.log when retained",
            job.display(),
            job.join("work").display(),
            job.exists()
        )
    })
}

fn run_producer(
    options: &EvidenceOptions,
    partition: Option<partitions::Partition>,
    runtime_inputs: Option<runtime_inputs::RuntimeInputs>,
    publication: Publication<'_>,
) -> CommandResult {
    if let Some(identity) = &runtime_inputs {
        identity.require_same(&runtime_inputs::RuntimeInputs::capture(
            publication.workspace.root(),
            options.producer,
        )?)?;
    }
    let spec = producer::prepare(options, publication.workspace.root(), partition.as_ref())?;
    ensure!(!spec.report.exists(), "producer report must start absent");
    let deadline = std::time::Instant::now() + Duration::from_secs(options.timeout_secs);
    let remaining = || -> Result<Duration> {
        deadline
            .checked_duration_since(std::time::Instant::now())
            .context("producer execution budget exhausted")
    };
    let version = execute_version(
        &spec.version,
        publication.workspace.root(),
        publication.root,
        remaining()?.min(Duration::from_secs(30)),
    )?;
    if let Some(tokens) = &spec.prerequisite {
        let _phase = crate::engines::process::phase::nested("baseline");
        let outcome = run_stage(tokens, remaining()?, "evidence", &publication)?;
        let (exit, output) = completed_outcome(outcome, options.producer)?;
        ensure!(exit == 0, "producer prerequisite did not pass: {output}");
        crate::engines::process::diagnostic(format_args!("{output}"));
    }
    for tokens in &spec.auxiliary {
        let outcome = run_stage(tokens, remaining()?, "evidence", &publication)?;
        let (exit, output) = completed_outcome(outcome, options.producer)?;
        ensure!(exit == 0, "producer report conversion failed: {output}");
    }
    let operation = if options.producer.kind() == EvidenceKind::Mutation {
        "mutation"
    } else {
        "evidence"
    };
    let outcome = run_stage(&spec.tokens, remaining()?, operation, &publication)?;
    let (exit, output) = completed_outcome(outcome, options.producer)?;
    crate::engines::process::diagnostic(format_args!("{output}"));
    publish(
        ProductionOutput {
            producer: options.producer,
            spec,
            version,
            exit,
            partition,
            runtime_inputs,
        },
        publication,
    )
}

fn run_stage(
    tokens: &[String],
    timeout: Duration,
    operation: &str,
    publication: &Publication<'_>,
) -> Result<crate::engines::process::ProcessOutcome> {
    let outcome = run_command_in_copy(
        tokens,
        (publication.workspace.root(), publication.root),
        timeout,
        operation,
    );
    publication.workspace.diagnostics(
        operation,
        &format!("command={tokens:?} outcome={outcome:?}"),
    )?;
    // Check both trees even after failures before accepting any output.
    publication.before.require_same(
        &Snapshot::capture_with(publication.workspace.root(), &publication.input_policy)?,
        "producer restoration",
    )?;
    publication.before.require_same(
        &Snapshot::capture_with(publication.root, &publication.input_policy)?,
        "checkout during producer execution",
    )?;
    Ok(outcome)
}
