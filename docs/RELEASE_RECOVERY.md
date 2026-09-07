# Release recovery

This runbook describes the checked-in 0.6 workflow. Local tests do not prove a
remote release, publisher activation, or registry state. Recovery uses the
existing authorization for the same signed release; unrelated publication or
external configuration needs its own authorized scope.

## Identity and checkpoints

The signed annotated tag identifies the payload commit. Recovery tooling comes
from the separately CI-validated `github.sha` checkout in `release-tooling/`.
Every receipt binds the version, source SHA, tooling SHA, signed tag object,
run ID, immutable bundle artifact ID, and archive digests. A tooling repair
cannot silently replace signed source or launcher bytes.

The six stages are:

| Stage | Required result |
| --- | --- |
| `version-check` | Signed tag, source versions, main-tip/recovery authorization, successful exact-source CI and artifact identity |
| `package` | Reused binaries for every supported native platform, reproducible archives, checksums, SBOM, and real offline npm/pnpm installs |
| `publish` | Checksum/SBOM attestations, receipt identity, authenticated prerequisites, publication of missing GitHub/crate/npm artifacts, and exact Cargo installation |
| `verify-exact` | Real npm/pnpm project/global checks and direct downloaded-binary checks for the exact release |
| `promote-channels` | All exact consumers verified, immutable versions unchanged, and independent readback of default selectors |
| `verify-channels` | Real default consumers for every channel and a complete receipt |

The bundle upload is the final packaging checkpoint. Attestation or publication
failures can therefore reuse it without rebuilding. `release-bundle` lasts 30
days. Attempt-specific publication, exact-consumer, promotion, and final
receipts last 90 days. Bundle and receipt stages pass exact artifact IDs directly. Native matrix jobs
retain attempt-numbered binaries and proofs. Collectors select each platform's
newest attempt within the same workflow run and verify source/target/receipt
identity; successful platforms may be retained from earlier partial attempts.

Eight channels must progress through `pending` → `staged` →
`immutable_verified` → `exact_consumer_verified` → `promoted` →
`default_consumer_verified`:

```text
hardgate-linux-x64
hardgate-linux-arm64
hardgate-darwin-x64
hardgate-darwin-arm64
hardgate-win32-x64
@tech-byte-frontier/hardgate
hardgate
github-assets
```

Transitions are adjacent, identity-bound, and replay-safe. Failures retain the
previous verified state. The final stage requires every channel to complete;
a failed, cancelled, or skipped dependency prevents it from succeeding.
Installed consumers verify binary bytes and full version/source identity,
then run actual local analysis on each matching host. The Linux x64 consumer
also runs complete `hardgate check`, including a real failing-test case and
input preservation. Native exact/default matrix failures block their aggregate
consumer checkpoint. A version response alone is insufficient.

## Retry or resume

Use **Re-run failed jobs** for ordinary recovery. The successful package job
and its immutable artifact remain available. Publication probes exact registry
versions first, verifies any existing bytes, and publishes only absent
artifacts. An ambiguous write requires an independent readback before another
attempt; mismatched or unresolved state blocks dependent actions.

If the workflow itself needs repair, dispatch the CI-validated workflow from
current `main` with the original `tag` and failed run's `resume_run_id`. The
workflow requires a completed failed tag-triggered run for that same signed
source, successful tag-validation and packaging checkpoints, exactly one
unexpired matching bundle, and successful CI for both source and recovery
tooling commits. It downloads that bundle instead of rebuilding its binary.
The new run records its own run/artifact identity and re-establishes channel
state from verified public bytes; previous receipts remain retained.

The 0.6 workflow rejects pre-0.6 payloads. Recover historical releases with
their original signed workflow and platform contract. Do not feed a historical
six-platform bundle into the new one-platform workflow, delete its published
assets, republish an existing version, or move its signed tag.

## Publication and promotion

GitHub stages a public prerelease without changing Latest. npm publishes all
platform packages, verifies them, then publishes the wrapper under
`hardgate-candidate`. crates.io exposes its immutable version independently;
this process does not make publication atomic across registries.

Promotion requires all eight channel exact-consumer checkpoints. npm's `latest` update
uses the separate token credential even when publication used trusted OIDC.
The crate's intended `max_stable_version` is verified without a registry
mutation. GitHub promotes only its byte-verified release. Default consumers
then independently verify the selected version and installed behavior.

## Missing or expired evidence

An expired bundle cannot be reconstructed automatically by `resume_run_id`.
Do not rebuild the tag and assume byte equality. Retain and reconcile the
original signed tag, same-tag public assets, checksums, SBOM, attestations,
receipts, exact registry metadata/bytes, and independent consumer evidence.
Missing identity, divergent bytes, ambiguous registry state, a moved default,
or an unverifiable signature blocks automated recovery. Preserve the evidence
and resolve that specific mismatch before further publication.

See [Publisher setup](PUBLISHER_SETUP.md) for credential boundaries and
[Maintainers](MAINTAINERS.md) for release preparation.
