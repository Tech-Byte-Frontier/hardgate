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

The workflow serializes release tags with the hardgate-release concurrency
group and promotes one verified bundle through these checkpoints:

1. version-check validates the signed source/tag, version identity, main
   ancestry, successful CI evidence, and recovery inputs.
2. build creates five cross-platform binaries; the native Linux x64 binary
   comes from the successful main CI artifact. package creates the six
   deterministic archives, checksums, build metadata, and SBOM, then runs
   archive and identity verification. The verified release-bundle is retained
   for 30 days.
3. attest verifies the signed tag and bundle again, then attests the
   checksums and archives/SBOM.
4. publication-preflight checks that GitHub and npm latest channels will not
   move backward and that the publication credentials authenticate.
5. github-release rechecks the tag and checksums. Existing expected assets
   must byte-match the bundle; missing expected assets may be uploaded, while
   unexpected assets, drafts, prereleases, or unknown release state stop the
   job.
6. publish-crates probes the exact crates.io version. An existing version
   must already match and is verified without republishing; a missing version
   is published once and then installed and checked for the exact version and
   source-commit identity.
7. publish-npm publishes the six platform packages in order and verifies
   each package against the bundle before publishing the
   @tech-byte-frontier/hardgate wrapper. The wrapper is deliberately last.
8. verify-channels checks the GitHub release assets, checksums, SBOM,
   archive identity, exact and latest registry versions, and clean Cargo,
   npm, pnpm, Yarn, Bun, and shell-installer consumers.
9. Release publication aggregate requires version-check, package, attest,
   publication-preflight, github-release, publish-crates, publish-npm, and
   verify-channels all to succeed. Any failure, cancellation, or skip fails
   the aggregate.

These checks establish artifact and channel identity; they do not make a
partially published npm release disappear. npm package versions are
immutable. A platform publication can therefore leave a partial state, with
the wrapper withheld until all six platform packages verify. The publication
helper probes an exact version before publishing, reconciles an ambiguous
publish response, and performs an independent byte/metadata verification.
Never blind-republish or treat an exit code alone as proof of publication.

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
present with different bytes, a registry response is ambiguous, the release
state cannot be determined, or the 30-day bundle has expired. A new release
must go through the normal signed-source and CI path.

## Pending operational work

The current workflow has run and artifact identifiers plus a 30-day bundle, but
it does not yet persist a durable release receipt. A durable receipt containing
source and tooling commits, CI/run and artifact identifiers, digests, and
per-channel states; explicit staged-promotion controls; recovery after bundle
retention expiry; and reviewed repository-rule, credential, and signer-rotation
procedures remain pending Phase 6 work. This guide describes the current
workflow and must not be read as claiming those controls are implemented.
