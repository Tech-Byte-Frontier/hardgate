// All six ordered checkpoints must succeed before completion is reachable.
"use strict";
import assert from "node:assert/strict";
import { release, releaseJob } from "./release_contract.sources.mjs";
const names = [...release.matchAll(/^  ([a-z][a-z0-9-]*):$/gm)].map((match) => match[1]).filter((name) => !["push", "workflow_dispatch"].includes(name));
assert.deepEqual(names, ["version-check", "package", "publish", "verify-exact", "promote-channels", "verify-channels"]);
const dependencies = Object.fromEntries(names.map((name) => [name, (releaseJob(name).match(/needs: \[([^\]]+)\]/)?.[1] ?? "").split(",").map((s) => s.trim()).filter(Boolean)]));
function completes(failed, state) {
  const result = {};
  for (const name of names) result[name] = dependencies[name].every((need) => result[need] === "success") ? (name === failed ? state : "success") : "skipped";
  return result["verify-channels"] === "success";
}
assert.equal(completes(null, null), true);
for (const name of names) {
  assert.doesNotMatch(releaseJob(name).split("    steps:\n")[0], /if:|continue-on-error/, "checkpoint success must use normal dependency semantics");
  for (const state of ["failure", "cancelled", "skipped"]) assert.equal(completes(name, state), false, `${name}=${state} cannot complete`);
}
assert.match(releaseJob("verify-channels"), /release-receipt-cli\.mjs assert --receipt receipt\/release\.json --require-complete/);
console.log("release_aggregate.test: six checkpoint dependency and receipt completion contracts verified");
