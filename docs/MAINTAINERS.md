# Maintainer guide

This document describes the current contribution, release-note, and recovery
contract. It follows the checked-in workflows in
.github/workflows/ci.yml and .github/workflows/release.yml.

## Contribution and release notes

Keep changes focused and source-backed. A pull request should state the
behavior change, the exact focused checks that ran, and the complete CI or
self-gate result when applicable. Reproductions should use the smallest
deterministic fixture and include the expected and actual result.

User-facing behavior changes belong in CHANGELOG.md under the release version.
Include configuration, CLI, API, migration, installation, and supported
platform implications when they apply. GitHub's generated notes are
supplementary; they do not replace curated migration notes. Keep README,
package metadata, and installer examples aligned with the channels that the
release workflow actually verifies.

## Release identity and readiness

The release payload is the commit named by the signed annotated vX.Y.Z tag.
version-check verifies the tag with .github/release-allowed-signers, checks
that the tag commit is the checked-out source, validates every version source,
and requires a successful CI quality aggregate run for that source commit.
The first attempt must target the current origin/main tip.

On a recovery dispatch, the signed tag remains the payload identity.
github.sha is the reviewed workflow/tooling commit used by the recovery
helpers in release-tooling; it must not silently replace files from the
tagged payload. The workflow binds recovery to a failed tag-triggered run,
the same tag and source commit, an unexpired verified release-bundle, and a
successful main CI run for the current main commit.

Before publication, confirm that the tag version matches Cargo.toml,
Cargo.lock, package.json, the wrapper, and all six platform package
manifests. The release workflow also checks that the six target packages are
exactly the advertised Linux/macOS set. Do not create a second release for an
unknown or partially observed state without first establishing which immutable
artifacts and registry versions already exist.

## Publication stages

The workflow serializes release tags with the `hardgate-release` concurrency
group. CI supplies the native Linux x64 binary; the release matrix builds the
other five targets. Packaging verifies the six deterministic archives,
checksums, metadata and SBOM, and installs the actual seven packed npm
artifacts with their optional dependencies before retaining the bundle.

A seeded receipt binds the signed source, CI-validated tooling, tag object,
run/artifact identifiers and archive digests. GitHub stages public prerelease
assets without changing Latest. The crate is published or independently
verified against the local clean Cargo archive, then installed by exact
version. npm establishes all six platform versions before publishing the
wrapper, using the `hardgate-candidate` tag.

Six native exact-version jobs verify package bytes and runnable identity;
the canonical GNU x64 job also verifies the signed wrapper and shell installer.
Promotion requires matching receipts proving all nine exact consumers. npm
Latest changes use separate token authentication and independent readback.
The crate default is verified without a registry mutation. GitHub then promotes
the byte-verified release to stable and Latest.

Six native default jobs and independent Cargo, npm, pnpm, Yarn, Bun, installer
and global-command consumers verify the default selectors. The aggregate
rejects every failed, cancelled or skipped prerequisite and requires a merged
receipt with all nine channels at `default_consumer_verified`. Partial receipts
and failure events are retained for recovery.

These checkpoints do not make publication atomic across registries. npm exact
versions and public GitHub prereleases remain accessible before promotion;
crates.io cannot hide a published stable version behind the same staging
mechanism. Existing immutable bytes must match before reuse. Ambiguous writes
require independent reconciliation before any retry.

See [release recovery](RELEASE_RECOVERY.md) for receipt states, reruns and
retention, and [publisher setup](PUBLISHER_SETUP.md) for authentication and
signer rotation. Checked-in workflow contracts are not evidence of an actual
remote release or publisher activation.

## Recovery

For an ordinary failed run, use GitHub Actions' Re-run failed jobs so the same
workflow and verified artifact checkpoint remain in use. If the workflow
definition needs a reviewed repair, dispatch it with the original tag and that
failed run's resume_run_id. The workflow will reject a run that is not the
matching failed tag run, lacks exactly one unexpired release-bundle, lacks the
required successful build/package checkpoints, or lacks successful CI for the
current main commit.

Resume reuses the verified bundle; it does not rebuild different bytes. Before
resuming after an npm failure, inspect exact package endpoints and run the
publication verifier for the packages already visible. Existing versions may
be accepted only when their manifests, platform constraints, executable mode,
and binary bytes match the bundle. Stop for maintainer review when a version is
present with different bytes, a registry response remains ambiguous after reconciliation, the release
state cannot be determined, or the 30-day bundle has expired. A new release
must go through the normal signed-source and CI path.

## External activation and retention limits

The checked-in repository-rule files are review-only proposals. Apply repository
protections, configure trusted publishers, rotate credentials/signers, or publish
only within the authorized scope and after exact-commit CI evidence. No local
helper test proves that external settings have been enabled.

Bundles expire after 30 days; receipts and native proofs expire after 90 days.
The automated `resume_run_id` path requires an unexpired bundle. Recovery after
expiry needs retained same-tag assets and independent identity, digest and
provenance evidence under the [recovery runbook](RELEASE_RECOVERY.md); it is not
an automatic rebuild path.
