# Release recovery

**Status:** review-only runbook for the proposed staged release workflow. It
does not prove remote workflow deployment, publisher activation, or GitHub,
npm, or crates.io state. Publication still requires explicit maintainer
authorization.

## Identity and retained evidence

The signed annotated release tag is the payload identity. `source_sha` is the
tag commit and `tooling_sha` is the separate CI-validated `github.sha` checkout
used by recovery helpers. A tooling fix may repair orchestration, but it must
not silently replace signed source files. Every receipt binds the version,
source and tooling commits, signed tag object, build run, bundle artifact, and
archive digests.

The verified `release-bundle` is retained for 30 days. Seeded and per-channel
receipts, native proofs, and failure records are retained for 90 days. Receipt
artifacts include the GitHub Actions run attempt in their names, so a rerun can
produce a newer candidate for one platform without deleting older evidence.
The central collector selects the newest valid attempt per native package and
preserves the older artifacts for audit.

The nine receipt channels are:

```text
hardgate-linux-x64, hardgate-linux-x64-musl, hardgate-linux-arm64,
hardgate-linux-arm64-musl, hardgate-darwin-x64, hardgate-darwin-arm64,
@tech-byte-frontier/hardgate, hardgate, github-assets
```

Their intended progression is `pending` → `staged` → `immutable_verified` →
`exact_consumer_verified` → `promoted` → `default_consumer_verified`.
Receipt transitions are adjacent, identity-bound, and replay-safe. A failure is
retained on the affected channel; it does not erase earlier evidence.

## Intended staged flow

1. `version-check`, CI, build, package, and attestation establish one verified
   six-archive bundle, checksums, SBOM, and source identity. The seed receipt
   is created only after the bundle is verified.
2. GitHub publishes the public prerelease assets with `latest=false`. The crate
   is established at its immutable exact version. npm publishes all six
   platform packages and then the wrapper under `hardgate-candidate`.
3. Six native exact jobs run on matching CPUs and ABIs. Linux musl jobs execute
   static-musl binaries on matching GNU Linux native-CPU runners; they inspect
   the static-musl ABI and native CPU rather than requiring a musl userspace.
   The jobs verify the archive and exact candidate consumer; the x64 GNU proof
   also covers the wrapper source. Each job applies its proof to a receipt and
   uploads its receipt and native proof under a run-attempt-specific name.
4. The collector merges the latest valid receipts until all nine channels are
   `exact_consumer_verified`. A missing or divergent identity blocks promotion.
5. Each npm candidate is promoted to `latest` once, with an independent
   exact-version/default-channel readback. The promotion uses the existing
   `secrets.NPM_TOKEN` even when candidate publication used trusted npm OIDC.
6. The crate publication remains an independent required channel. Verify its
   intended `max_stable_version` and exact installed identity; npm or GitHub
   success cannot hide a missing or ambiguous crate publication.
7. GitHub changes the verified prerelease to the stable/latest release state,
   then independently verifies the stable assets and latest pointer.
8. Six native default jobs verify the default channel on the matching runners.
   The canonical GNU x64 job also checks the shell installer with an unset
   version for `latest`; exact mode uses the signed `vX.Y.Z` explicitly. The
   installed bytes and full `hardgate VERSION (COMMIT)` identity must match.
9. Independent default consumers run for the crate, npm, pnpm, Yarn, Bun,
   shell installer, and global command paths. The final receipt merge uses
   `require-complete` and succeeds only when all nine channels reach
   `default_consumer_verified`.

The proposed workflow uses the receipt and staging helpers under `scripts/`.
Do not report this sequence as deployed until the signed `main` workflow, its
successful CI run, retained artifacts, and public readbacks prove each
checkpoint.

## Ordinary recovery

For a failed job, inspect the retained receipt and failure event, then rerun the
failed job when the same immutable inputs remain available. Native reruns may
replace the collector's selected attempt for that package, but older receipts
and proofs remain retained. Do not republish an immutable npm version, overwrite
matching GitHub assets, or roll back a channel. Independently reconcile an
ambiguous write from its receipt and public state within the authorized recovery
scope; stop only when identity, integrity, or authorization remains unresolved.

`resume_run_id` is a narrow same-tag recovery input. The current workflow binds
it to a completed failed tag-triggered run, the same signed tag and source
commit, the required successful checkpoints, an unexpired matching
`release-bundle`, and successful CI for the current main commit. It reuses the
verified bytes and the same signed source. A recovery run may use a new
CI-validated tooling commit, which must be recorded and checked separately; it
does not authorize a different signed source or a rebuild of the verified
payload.

## Expired-artifact recovery

When the 30-day bundle has expired, the current `resume_run_id` path cannot
reconstruct it automatically. Do not rebuild from the tag and assume the bytes
are equivalent. Stop the automated recovery and assemble manual evidence for
maintainer review:

- the same signed tag and source commit, plus the CI-validated tooling commit;
- retained same-tag GitHub assets and their checksums, SBOM, attestations, and
  release metadata;
- retained seed/channel receipts, native proofs, and failure history;
- exact public crate and npm metadata/bytes, candidate/default dist-tags, and
  GitHub release state; and
- independent consumer evidence showing the requested version and full binary
  source identity.

If any required identity, digest, receipt, or public-state evidence is missing
or ambiguous, do not blind-rebuild or overwrite the channel. A maintainer must
choose a documented evidence-based recovery or start a new signed release path.

## Stop conditions

Stop before the next publication or promotion when the signed tag cannot be
verified, either the signed source identity or the CI tooling identity fails
its recorded expected binding, an artifact is expired or has unexpected bytes,
a receipt merge diverges, a registry state remains ambiguous after independent
reconciliation, the crate
`max_stable_version` does not match, or a default consumer fails. Preserve all
receipts and failure artifacts and record the public state that caused the
stop.

For npm credential boundaries, see [Publisher setup](PUBLISHER_SETUP.md). For
the checked-in workflow's existing recovery guards, see the
[maintainer guide](MAINTAINERS.md); neither document turns the local proposal
into deployed behavior.
