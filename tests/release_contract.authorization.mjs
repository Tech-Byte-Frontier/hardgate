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
const versionCheckJob = release.slice(release.indexOf("  version-check:"), release.indexOf("  quality-gate:"));
includesAll(
  versionCheckJob,
  [
    "GH_TOKEN: ${{ github.token }}",
    "RUN_ATTEMPT: ${{ github.run_attempt }}",
    'verify-tag "$RELEASE_TAG"',
    "refs/remotes/origin/main",
    'if [ "$RUN_ATTEMPT" -le 1 ]',
    'git merge-base --is-ancestor "$tag_commit" "$main_commit"',
    "is recovering signed release",
    "actions/workflows/ci.yml/runs",
    "-X GET",
    "-f branch=main",
    "-f event=push",
    "-f status=success",
    '-f head_sha="$tag_commit"',
    "-f per_page=1",
    "release commit has no completed successful main CI run",
  ],
  "signed main-tip release precondition",
);
assert.match(
  versionCheckJob,
  /if \[ "\$tag_commit" != "\$main_commit" \]; then\s+if \[ "\$RUN_ATTEMPT" -le 1 \] \|\| ! git merge-base --is-ancestor "\$tag_commit" "\$main_commit"; then/,
  "only an ancestor-tag rerun may recover after main advances",
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
    "needs: [version-check, quality-gate]",
    "CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}",
    "CARGO_REGISTRY_TOKEN is required for crates.io publication",
    "NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}",
    "npm whoami --registry=https://registry.npmjs.org",
  ],
  "publication preflight",
);
assert.match(
  githubReleaseJob,
  /needs: \[version-check, quality-gate, package, attest, publication-preflight\]/,
  "GitHub publication must wait for attestation and registry preflight",
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
