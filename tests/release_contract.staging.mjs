// Cross-job evidence and credential boundaries for staged publication.
"use strict";

import assert from "node:assert/strict";
import { includesAll, platformPackages, releaseJob } from "./release_contract.sources.mjs";

function requireDependencies(name, expected) {
  const job = releaseJob(name);
  const actual = job.match(/^    needs: \[([^\]]+)\]/m)?.[1].split(",").map((value) => value.trim());
  assert.deepEqual(new Set(actual), new Set(expected), `${name} must wait for every evidence prerequisite`);
  for (const dependency of expected) {
    assert.ok(job.includes(`needs.${dependency}.result == 'success'`), `${name} cannot accept skipped ${dependency}`);
  }
  return job;
}

const exact = requireDependencies("verify-native-exact", ["version-check", "package", "publish-npm"]);
const promotion = requireDependencies("promote-channels", ["version-check", "package", "publish-npm", "verify-native-exact"]);
const defaults = requireDependencies("verify-native-default", ["version-check", "package", "promote-channels"]);
const consumers = requireDependencies("verify-channels", ["version-check", "package", "promote-channels", "verify-native-default"]);

for (const [phase, job] of [["exact", exact], ["default", defaults]]) {
  const packages = [...job.matchAll(/^          - package: (\S+)$/gm)].map((match) => match[1]);
  assert.deepEqual(new Set(packages), new Set(platformPackages), `${phase} requires all six native platforms`);
  assert.equal(packages.length, platformPackages.length, `${phase} cannot duplicate a platform`);
  includesAll(job, [
    "fail-fast: false", "contents: read", "actions: read", "digest-mismatch: error",
    `NATIVE_MODE: ${phase}`, '--mode "$NATIVE_MODE"', "scripts/verify-native-channel.mjs", "scripts/apply-native-receipt.mjs",
    'verify-tag "$RELEASE_TAG"', "--wrapper-source npm/hardgate/bin/hardgate.js",
    `release-receipt-${phase}-`, "github.run_attempt", "retention-days: 90",
    "if: always() && hashFiles('receipt/release.json') != ''",
  ], `${phase} native evidence`);
  assert.doesNotMatch(job, /secrets\.|id-token: write|contents: write/, `${phase} consumers must not receive publish credentials`);
}

includesAll(promotion, [
  "--phase exact", "scripts/merge-release-receipts.mjs", "exact-receipts/*/release.json",
  'length == 9', "exact_consumer_verified", "scripts/promote-npm-channels.mjs",
  "NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}", "--require-default", "--channel hardgate --to promoted",
  "scripts/promote-github-channel.mjs", "release-receipt-promoted-attempt-",
  "if: always() && hashFiles('receipt/release.json') != ''",
], "all-channel promotion barrier");
assert.doesNotMatch(promotion, /--to default_consumer_verified|id-token: write/, "promotion records only promotion and has explicit token auth");
assert.ok(promotion.indexOf("exact-receipts/*/release.json") < promotion.indexOf("scripts/promote-npm-channels.mjs"));
assert.ok(promotion.indexOf("scripts/promote-npm-channels.mjs") < promotion.indexOf("scripts/promote-github-channel.mjs"));

includesAll(consumers, ["--require-default", "--to default_consumer_verified", "--consumer", "@tech-byte-frontier/hardgate@latest", '"$pnpm_tool" bin --global', "command -v hardgate"], "fresh default installs");
assert.doesNotMatch(consumers, /cargo install hardgate --version|@tech-byte-frontier\/hardgate@\$\{RELEASE_VERSION\}/, "default consumers must resolve the default selector");
const aggregate = releaseJob("release-complete");
includesAll(aggregate, ["--phase default", "default-receipts/*/release.json", "consumer-receipt/release.json", "--require-complete", "release-receipt-complete-attempt-"], "authoritative receipt completion");

const packaging = releaseJob("package");
includesAll(packaging, ["npm pack --ignore-scripts --pack-destination", "scripts/check-packed-consumers.mjs", "--packages-dir", "--binary", "PNPM_VERSION"], "actual packed optional-dependency acceptance");
assert.ok(packaging.indexOf("scripts/check-packed-consumers.mjs") < packaging.indexOf("- id: upload_bundle"), "consumer acceptance must precede release bundle publication");

const npm = releaseJob("publish-npm");
includesAll(npm, ["--to staged", "scripts/publish-npm-package.mjs", "--to immutable_verified", "release-receipt-npm-attempt-"], "npm staging receipts");
assert.doesNotMatch(npm, /npm dist-tag|promote-npm-channels/, "candidate publication must not promote defaults");
for (const [name, mutation] of [["github-release", "scripts/stage-github-release.mjs"], ["publish-crates", "cargo publish --locked"], ["publish-npm", "scripts/publish-npm-package.mjs"]]) {
  const job = releaseJob(name);
  const identity = job.indexOf("Verify receipt identity before publication");
  assert.ok(identity >= 0 && identity < job.indexOf(mutation), `${name} must validate the full receipt binding before publication`);
  includesAll(job.slice(identity, job.indexOf(mutation)), ["create --output receipt/release.json", "--tooling-sha", "--source-sha", "--tag-object", "--build-run-id", "--artifact-id", "--dist dist"], `${name} identity and archive binding`);
}
includesAll(npm, ["if: failure() && hashFiles('receipt/release.json') != ''", "channel='@tech-byte-frontier/hardgate'"], "preparation failures retain a blocked wrapper checkpoint");
const preflight = releaseJob("publication-preflight");
includesAll(preflight, ["HARDGATE_NPM_AUTH_MODE == 'trusted'", "--auth-mode token"], "separate trusted publication and promotion credentials");
