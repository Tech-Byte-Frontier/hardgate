import assert from "node:assert/strict";
import {
  API_PAYLOAD_KEYS,
  validateApiPayload,
  validateReviewNotes,
  validateRuleProposals,
  validateWorkflowContract,
  verifyRepositoryRules,
} from "../scripts/verify-repository-rules.mjs";

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

const result = verifyRepositoryRules();
assert.equal(result.aggregateName, "CI quality aggregate");
assert.deepEqual(
  result.needs,
  ["rust", "npm-wrapper", "npm-wrapper-minimum", "macos-process-restoration", "repository-rules", "hardgate-self", "release-contract"],
);
assert.equal(result.proposals.length, 2);

for (const { file, payload, review } of result.proposals) {
  assert.deepEqual(Object.keys(payload).sort(), [...API_PAYLOAD_KEYS].sort(), `${file} must export only API payload fields`);
  assert.equal(payload.enforcement, "disabled", `${file} must not activate remotely`);
  assert.deepEqual(payload.bypass_actors, [], `${file} must not contain invented bypass actors`);
  assert.equal(review.status, "REVIEW-ONLY", `${file} review notes must remain review-only`);
  assert.ok(review.recovery.steps.length >= 2, `${file} needs maintainer recovery steps`);
}

const main = result.proposals.find(({ payload }) => payload.target === "branch");
const tags = result.proposals.find(({ payload }) => payload.target === "tag");
assert.equal(main.payload.rules.find((rule) => rule.type === "required_status_checks").parameters.required_status_checks[0].context, result.aggregateName);
assert.ok(tags.payload.rules.some((rule) => rule.type === "update"));
assert.ok(!tags.payload.rules.some((rule) => rule.type === "required_signatures"));
assert.match(tags.review.signed_tag_authorization, /release preflight/);
assert.match(tags.review.signed_tag_authorization, /does not verify tag objects/);

const wrongAggregate = clone(main);
wrongAggregate.payload.rules.find((rule) => rule.type === "required_status_checks").parameters.required_status_checks[0].context = "Wrong aggregate";
assert.throws(
  () => validateRuleProposals({ aggregateName: result.aggregateName, proposals: [wrongAggregate, tags] }),
  /must require CI quality aggregate/,
);

const missingPrerequisite = { ...result, needs: result.needs.filter((job) => job !== "macos-process-restoration") };
assert.throws(() => validateWorkflowContract(missingPrerequisite), /macos-process-restoration is missing/);
assert.throws(() => validateWorkflowContract({ ...result, aggregateName: "CI / wrong aggregate" }), /workflow aggregate name is not stable/);

const enabled = clone(main);
enabled.payload.enforcement = "active";
assert.throws(() => validateApiPayload(enabled.payload, "enabled proposal"), /must remain disabled/);

const malformed = clone(main);
delete malformed.payload.conditions;
assert.throws(() => validateRuleProposals({ aggregateName: result.aggregateName, proposals: [malformed, tags] }), /contain only GitHub API payload fields/);

const unsupportedTagClaim = clone(tags.review);
unsupportedTagClaim.signed_tag_authorization = "the ruleset verifies tag objects";
assert.throws(() => validateReviewNotes(unsupportedTagClaim, tags.file, "tag"), /release preflight/);

console.log("repository_rules.test: OK");
