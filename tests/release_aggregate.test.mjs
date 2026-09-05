// Evaluate the actual aggregate rejection condition over every prerequisite.
"use strict";
import assert from "node:assert/strict";
import { release } from "./release_contract.sources.mjs";

const aggregate = release.slice(release.indexOf("  release-complete:"));
const dependencies = aggregate.match(/needs: \[([^\]]+)\]/)[1].split(",").map((value) => value.trim());
const condition = aggregate.match(/if: \$\{\{ (contains[^\n]+) \}\}/)[1];
const statuses = ["success", "failure", "cancelled", "skipped"];

function rejected(results) {
  const clauses = condition.split(" || ");
  return clauses.some((clause) => {
    const status = clause.match(/^contains\(needs\.\*\.result, '([^']+)'\)$/)?.[1];
    assert.ok(status, "unrecognized aggregate expression requires an updated evaluator");
    return results.includes(status);
  });
}

assert.deepEqual(new Set(dependencies), new Set(["version-check", "package", "attest", "publication-preflight", "receipt-init", "github-release", "publish-crates", "publish-npm", "verify-native-exact", "promote-channels", "verify-native-default", "verify-channels"]), "every staging, promotion, and consumer checkpoint must be required");
assert.equal(rejected(dependencies.map(() => "success")), false);
for (const prerequisite of dependencies) {
  for (const status of statuses.slice(1)) {
    const results = dependencies.map((job) => job === prerequisite ? status : "success");
    assert.equal(rejected(results), true, `${prerequisite}=${status} cannot produce green completion`);
  }
}
assert.equal(rejected(dependencies.map((job) => job === "version-check" ? "success" : "skipped")), true, "historical green-but-unpublished recovery must fail");
console.log("release_aggregate.test: OK");
