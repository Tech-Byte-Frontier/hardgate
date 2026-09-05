import assert from "node:assert/strict";
import { verifyRepositoryRules } from "../scripts/verify-repository-rules.mjs";

const result = verifyRepositoryRules();
assert.equal(result.aggregateName, "CI quality aggregate");
assert.deepEqual(
  result.needs,
  ["rust", "npm-wrapper", "npm-wrapper-minimum", "macos-process-restoration", "repository-rules", "hardgate-self", "release-contract"],
);
assert.equal(result.proposals.length, 2);

for (const { file, value } of result.proposals) {
  assert.equal(value.enforcement, "disabled", `${file} must not activate remotely`);
  assert.deepEqual(value.bypass_actors, [], `${file} must not contain invented bypass actors`);
  assert.equal(value._proposal.status, "REVIEW-ONLY", `${file} must remain review-only`);
  assert.ok(value._proposal.recovery.steps.length >= 2, `${file} needs maintainer recovery steps`);
}

const tags = result.proposals.find(({ value }) => value.target === "tag").value;
assert.ok(tags.rules.some((rule) => rule.type === "required_signatures"));
assert.deepEqual(tags.conditions.ref_name.include, ["refs/tags/v*"]);

console.log("repository_rules.test: OK");
