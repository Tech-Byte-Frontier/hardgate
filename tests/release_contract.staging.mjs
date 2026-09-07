// Ordered release checkpoints preserve immutable evidence and credential scope.
"use strict";
import assert from "node:assert/strict";
import { includesAll, installedConsumers, installedCheck, releaseJob } from "./release_contract.sources.mjs";

const requirements = {
  package: ["version-check"],
  publish: ["version-check", "package"],
  "verify-exact": ["version-check", "package", "publish", "native-exact"],
  "promote-channels": ["version-check", "package", "verify-exact"],
  "verify-channels": ["version-check", "package", "promote-channels", "native-default"],
};
for (const [name, expected] of Object.entries(requirements)) {
  const job = releaseJob(name);
  const actual = job.match(/^    needs: \[([^\]]+)\]/m)?.[1].split(",").map((value) => value.trim());
  assert.deepEqual(actual, expected, `${name} must retain every checkpoint prerequisite`);
  assert.doesNotMatch(job.split("    steps:\n")[0], /always\(\)|continue-on-error/, "normal success semantics must reject failed or skipped dependencies");
}
for (const [name, phase] of [["verify-exact", "exact"], ["verify-channels", "default"]]) {
  const job = releaseJob(name);
  includesAll(job, ["digest-mismatch: error", `NATIVE_MODE: ${phase}`, "scripts/verify-native-channel.mjs",
    `scripts/release-consumers.sh ${phase}`, `scripts/release-direct-consumer.sh ${phase}`,
    "scripts/apply-native-receipt.mjs", "--wrapper-source npm/hardgate/bin/hardgate.js",
    "retention-days: 90", "if: always() && hashFiles('receipt/release.json') != ''"], `${phase} consumer evidence`);
  assert.ok(job.indexOf(`release-consumers.sh ${phase}`) < job.indexOf("scripts/apply-native-receipt.mjs"));
  assert.doesNotMatch(job, /secrets\.|id-token: write|contents: write/);
}
includesAll(installedConsumers, ['exact) selector="${RELEASE_VERSION:', "default) selector=latest", '"$pnpm_tool" bin --global', "command -v hardgate", '"$acceptance_script" "$command"'], "real installed checks for project and global consumers");
includesAll(installedCheck, ['["check", "--json"]', "report.accepted", 'engine.state === "completed"', "inputHashes", 'finding.step === "test"'], "real check result, engine completion, input preservation, and test failures");
const promotion = releaseJob("promote-channels");
includesAll(promotion, ["needs.verify-exact.outputs.receipt_artifact_id", "scripts/merge-release-receipts.mjs", "length == 7", "exact_consumer_verified", "scripts/promote-npm-channels.mjs", "--require-default", "--channel hardgate --to promoted", "scripts/promote-github-channel.mjs"], "all-channel promotion barrier");
assert.ok(promotion.indexOf("length == 7") < promotion.indexOf("scripts/promote-npm-channels.mjs"));
assert.ok(promotion.indexOf("scripts/promote-npm-channels.mjs") < promotion.indexOf("scripts/promote-github-channel.mjs"));
assert.doesNotMatch(promotion, /--to default_consumer_verified/);
const packaging = releaseJob("package");
assert.ok(packaging.indexOf("scripts/check-packed-consumers.mjs") < packaging.indexOf("- id: upload_bundle"));
const publish = releaseJob("publish");
const identity = publish.indexOf("Bind the receipt to signed source and verified bundle");
for (const mutation of ["scripts/stage-github-release.mjs", "cargo publish --locked", "scripts/publish-npm-package.mjs"]) {
  assert.ok(identity >= 0 && identity < publish.indexOf(mutation));
}
includesAll(publish, ["--tooling-sha", "--source-sha", "--tag-object", "--build-run-id", "--artifact-id", "--dist dist", "--to staged", "--to immutable_verified", "receipt/active-channel"], "immutable publication identity and partial receipts");
assert.doesNotMatch(publish, /npm dist-tag|promote-npm-channels/);
const consumers = releaseJob("verify-channels");
includesAll(consumers, ["--require-default", "--to default_consumer_verified", "installed-check.mjs", "--require-complete"], "complete default consumers");
assert.ok(consumers.indexOf("--to default_consumer_verified") < consumers.indexOf("--require-complete"));
