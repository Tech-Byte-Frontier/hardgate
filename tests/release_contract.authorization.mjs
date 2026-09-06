// Static authorization and publication-precondition assertions.
"use strict";

import assert from "node:assert/strict";
import {
  includesAll,
  release,
  releaseJob,
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
const publishJob = releaseJob("publish");
const versionCheckJob = releaseJob("version-check");
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

includesAll(publishJob, [
  "needs: [version-check, package]", "CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}",
  "NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}", "node release-tooling/scripts/npm-publisher-preflight.mjs",
  "HARDGATE_NPM_AUTH_MODE == 'trusted'", "--auth-mode token", "id-token: write", "contents: write",
  "attestations: write", "artifact-metadata: write",
], "publication authorization");
const preflight = publishJob.indexOf("name: Validate npm channel promotion authentication");
assert.ok(preflight >= 0 && preflight < publishJob.indexOf("scripts/stage-github-release.mjs"));
for (const marker of ["Recheck signed tag immediately before GitHub", "Recheck signed tag immediately before crates.io", "Recheck signed tag immediately before npm"]) {
  assert.ok(publishJob.includes(marker), "each channel rechecks the signed tag");
}
for (const job of ["version-check", "package", "verify-exact", "verify-channels"]) {
  assert.doesNotMatch(releaseJob(job), /secrets\.|id-token: write|contents: write/, `${job} must not receive publication credentials`);
}
assert.doesNotMatch(releaseJob("promote-channels"), /id-token: write/, "dist-tag promotion uses explicit token authentication");
includesAll(publishJob, [
  'test "$(git -C release-tooling rev-parse HEAD)" = "$GITHUB_SHA"',
  'node release-tooling/scripts/stage-github-release.mjs --repo "$GITHUB_REPOSITORY" --tag "$RELEASE_TAG" --version "$RELEASE_VERSION" --dist dist',
], "tested staging and immutable existing-release recovery");
