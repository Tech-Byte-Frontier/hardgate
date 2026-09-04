// Static authorization and publication-precondition assertions.
"use strict";

import assert from "node:assert/strict";
import {
  includesAll,
  release,
  releaseAllowedSigners,
} from "./release_contract.sources.mjs";

assert.match(release, /permissions:\s*\n\s+contents: read/, "release workflow default token must be read-only");
assert.match(
  release,
  /^permissions:\n  contents: read\n  actions: read$/m,
  "release preconditions need read-only Actions API access",
);
assert.match(release, /concurrency:[\s\S]*?group: hardgate-release\s/, "all release tags must share one publication lock");
assert.doesNotMatch(release, /group: hardgate-release-\$\{\{/, "release concurrency must not be isolated per tag");
assert.match(
  release,
  /concurrency:\n(?:  #[^\n]*\n)*  group: hardgate-release\n  cancel-in-progress: false/,
  "an in-flight publication must never be cancelled by another tag",
);
assert.match(release, /attest:[\s\S]*?permissions:[\s\S]*?attestations: write/, "only the resumable attestation job may attest artifacts");
assert.match(release, /github-release:[\s\S]*?permissions:[\s\S]*?contents: write/, "only GitHub publication may write contents");
assert.match(release, /publish-npm:[\s\S]*?permissions:[\s\S]*?id-token: write/, "npm provenance publication requires scoped OIDC access");

const githubReleaseJob = release.slice(release.indexOf("  github-release:"), release.indexOf("  publish-crates:"));
const publishCratesJob = release.slice(release.indexOf("  publish-crates:"), release.indexOf("  publish-npm:"));
const publishNpmJob = release.slice(release.indexOf("  publish-npm:"), release.indexOf("  verify-channels:"));
const attestJob = release.slice(release.indexOf("  attest:"), release.indexOf("  publication-preflight:"));
const verifyChannelsJob = release.slice(release.indexOf("  verify-channels:"), release.indexOf("  release-complete:"));
const releaseCompleteJob = release.slice(release.indexOf("  release-complete:"));
const versionCheckJob = release.slice(release.indexOf("  version-check:"), release.indexOf("  build:"));
includesAll(
  versionCheckJob,
  [
    "GH_TOKEN: ${{ github.token }}",
    "RUN_ATTEMPT: ${{ github.run_attempt }}",
    "RESUME_RUN_ID: ${{ inputs.resume_run_id }}",
    'verify-tag "$RELEASE_TAG"',
    "refs/remotes/origin/main",
    'if [ "$resume" != true ] && [ "$RUN_ATTEMPT" -le 1 ]',
    'git merge-base --is-ancestor "$tag_commit" "$main_commit"',
    "recovering signed release",
    "actions/workflows/ci.yml/runs",
    "-X GET",
    "-f branch=main",
    "-f event=push",
    "-f status=success",
    '-f head_sha="$tag_commit"',
    "-f per_page=1",
    "release commit has no completed successful main CI run",
    'if [ "$EVENT_NAME" = workflow_dispatch ] && [ "$GITHUB_SHA" != "$main_commit" ]',
    "resume workflow commit has no completed successful main CI run",
    'startswith("native-linux-x64-attempt-")',
    "max_by(.id)",
    "ci_native_artifact_id",
    "ci_run_id",
    'if [ "$EVENT_NAME" != workflow_dispatch ]',
    'if [ "$RESUME_RUN_ID" = "$CURRENT_RUN_ID" ]',
    '(.path | split("@")[0]) == ".github/workflows/release.yml"',
    '.event == "push"',
    '.conclusion == "failure"',
    '"Package and verify release artifacts"',
    '.name == "release-bundle"',
    '.expired == false',
    ".workflow_run.head_sha == $sha",
    ".workflow_run.id == $run",
    "resume_artifact_id",
  ],
  "signed main-tip and artifact-bound resume precondition",
);
assert.match(
  versionCheckJob,
  /if \[ "\$tag_commit" != "\$main_commit" \]; then\s+if ! git merge-base --is-ancestor "\$tag_commit" "\$main_commit"; then[\s\S]*?if \[ "\$resume" != true \] && \[ "\$RUN_ATTEMPT" -le 1 \]; then/,
  "only a verified bundle resume or an ancestor-tag rerun may recover after main advances",
);
const versionPreconditionOrder = [
  'verify-tag "$RELEASE_TAG"',
  'tag_commit=$(git rev-parse "${RELEASE_TAG}^{commit}")',
  "git fetch --no-tags origin",
  'if [ "$tag_commit" != "$main_commit" ]',
  "actions/workflows/ci.yml/runs",
  "echo \"tag=$RELEASE_TAG\"",
].map((snippet) => versionCheckJob.indexOf(snippet));
assert.ok(
  versionPreconditionOrder.every((position, index) => position >= 0 && (index === 0 || position > versionPreconditionOrder[index - 1])),
  "signature, main-tip, and successful-CI checks must precede release outputs",
);

const activeAllowedSigners = releaseAllowedSigners
  .split(/\r?\n/)
  .map((line) => line.trim())
  .filter((line) => line && !line.startsWith("#"));
assert.equal(activeAllowedSigners.length, 1, "release signer allowlist must contain exactly one active key");
assert.match(
  activeAllowedSigners[0],
  /^\S+ ssh-(?:rsa|ed25519) [A-Za-z0-9+/]+={0,3}$/,
  "release signer allowlist must contain a principal and a valid SSH public-key record",
);

const publicationPreflightJob = release.slice(
  release.indexOf("  publication-preflight:"),
  release.indexOf("  github-release:"),
);
includesAll(
  publicationPreflightJob,
  [
    "needs: [version-check]",
    "CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}",
    "CARGO_REGISTRY_TOKEN is required for crates.io publication",
    "NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}",
    "npm whoami --registry=https://registry.npmjs.org",
  ],
  "publication preflight",
);
assert.match(
  githubReleaseJob,
  /needs: \[version-check, package, attest, publication-preflight\]/,
  "GitHub publication must wait for attestation and registry preflight",
);
for (const [label, job, requiredNeeds] of [
  ["attestation", attestJob, ["version-check", "package"]],
  ["GitHub Release", githubReleaseJob, ["version-check", "package", "attest", "publication-preflight"]],
  ["crates.io", publishCratesJob, ["version-check", "package", "github-release"]],
  ["npm", publishNpmJob, ["version-check", "package", "github-release", "publish-crates"]],
  ["channel verification", verifyChannelsJob, ["version-check", "github-release", "publish-crates", "publish-npm"]],
]) {
  assert.match(job, /if: >-\s+\$\{\{\s+always\(\)/, `${label} must override intentional resume skip propagation`);
  for (const dependency of requiredNeeds) {
    assert.ok(job.includes(`needs.${dependency}.result == 'success'`), `${label} must require successful ${dependency}`);
  }
}
includesAll(
  releaseCompleteJob,
  [
    "name: Release publication aggregate",
    "if: ${{ always() }}",
    "needs: [version-check, package, attest, publication-preflight, github-release, publish-crates, publish-npm, verify-channels]",
    "contains(needs.*.result, 'failure')",
    "contains(needs.*.result, 'cancelled')",
    "contains(needs.*.result, 'skipped')",
  ],
  "release completion aggregate",
);
for (const [label, job] of [
  ["GitHub Release", githubReleaseJob],
  ["crates.io", publishCratesJob],
  ["npm", publishNpmJob],
]) {
  includesAll(
    job,
    [
      "Recheck signed tag immediately before",
      "RELEASE_TAG: ${{ needs.version-check.outputs.tag }}",
      "RELEASE_COMMIT: ${{ needs.version-check.outputs.commit }}",
      'git cat-file -t "$RELEASE_TAG"',
      'verify-tag "$RELEASE_TAG"',
      'git rev-parse "${RELEASE_TAG}^{commit}"',
    ],
    `${label} publication tag guard`,
  );
}
includesAll(
  githubReleaseJob,
  [
    "--json tagName,isDraft,isPrerelease",
    'test "$release_tag" = "$RELEASE_TAG"',
    'test "$release_is_draft" = false',
    'test "$release_is_prerelease" = false',
    "refusing to mutate it",
  ],
  "existing GitHub release state guard",
);
