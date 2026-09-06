import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const API_PAYLOAD_KEYS = ["bypass_actors", "conditions", "enforcement", "name", "rules", "target"];
const REQUIRED_NEEDS = ["rust", "npm-wrapper", "npm-wrapper-minimum", "repository-rules", "hardgate-self", "release-contract"];
const PROPOSAL_FILES = [
  { payload: "main-branch.review-only.json", review: "main-branch.review.json" },
  { payload: "version-tags.review-only.json", review: "version-tags.review.json" },
];

function fail(message) {
  throw new Error(`repository rules: ${message}`);
}

function requireValue(condition, message) {
  if (!condition) fail(message);
}

function jobBlock(workflow, jobId) {
  const lines = workflow.split(/\r?\n/);
  const start = lines.findIndex((line) => line === `  ${jobId}:`);
  requireValue(start >= 0, `workflow is missing jobs.${jobId}`);
  const block = [];
  for (const line of lines.slice(start + 1)) {
    if (/^  [A-Za-z0-9_-]+:\s*$/.test(line)) break;
    block.push(line);
  }
  return block.join("\n");
}

function scalar(block, key) {
  const match = block.match(new RegExp(`^    ${key}:\\s*(.+)$`, "m"));
  requireValue(match, `quality job is missing ${key}`);
  return match[1].trim().replace(/^(?:"([^"]*)"|'([^']*)')$/, "$1$2");
}

function inlineList(block, key) {
  const match = block.match(new RegExp(`^    ${key}:\\s*\\[([^\\]]*)\\]$`, "m"));
  requireValue(match, `quality job is missing inline ${key}`);
  return match[1].split(",").map((value) => value.trim()).filter(Boolean);
}

function readWorkflowContract(root = ROOT) {
  const workflow = fs.readFileSync(path.join(root, ".github/workflows/ci.yml"), "utf8");
  const quality = jobBlock(workflow, "quality");
  return { aggregateName: scalar(quality, "name"), needs: inlineList(quality, "needs") };
}

export function validateWorkflowContract(workflow) {
  requireValue(workflow.aggregateName === "CI quality aggregate", "workflow aggregate name is not stable");
  for (const job of REQUIRED_NEEDS) {
    requireValue(workflow.needs.includes(job), `${job} is missing from the CI aggregate prerequisites`);
  }
}

function findRule(payload, type) {
  return payload.rules.find((rule) => rule.type === type);
}

export function validateApiPayload(payload, label = "proposal") {
  requireValue(payload && typeof payload === "object" && !Array.isArray(payload), `${label} must be an object`);
  const keys = Object.keys(payload).sort();
  requireValue(JSON.stringify(keys) === JSON.stringify([...API_PAYLOAD_KEYS].sort()), `${label} must contain only GitHub API payload fields`);
  requireValue(payload.enforcement === "disabled", `${label} must remain disabled`);
  requireValue(Array.isArray(payload.bypass_actors) && payload.bypass_actors.length === 0, `${label} must not invent bypass actors`);
  requireValue(Array.isArray(payload.rules), `${label} must declare rules`);
}

export function validateReviewNotes(review, expectedPayload, expectedTarget) {
  requireValue(review?.status === "REVIEW-ONLY", `${expectedPayload} must be review-only`);
  requireValue(review.api_payload === expectedPayload, `${expectedPayload} review metadata points elsewhere`);
  requireValue(typeof review.api_submission === "string" && review.api_submission.includes("disabled"), `${expectedPayload} needs disabled submission guidance`);
  requireValue(review.recovery?.maintainer === "repository administrator", `${expectedPayload} needs an explicit recovery maintainer`);
  requireValue(review.recovery.steps?.length >= 2, `${expectedPayload} needs recovery steps`);
  if (expectedTarget === "tag") {
    const authorization = review.signed_tag_authorization ?? "";
    requireValue(authorization.includes("release preflight"), `${expectedPayload} must delegate signed-tag authorization to release preflight`);
    requireValue(authorization.includes("git verify-tag"), `${expectedPayload} must name tag verification`);
    requireValue(authorization.includes("allowed-signers"), `${expectedPayload} must name the allowed-signers file`);
    requireValue(authorization.includes("does not verify tag objects"), `${expectedPayload} must not claim ruleset tag-object verification`);
  }
}

function validateCommon(proposal, expectedTarget, expectedRef) {
  const { file, payload, review } = proposal;
  validateApiPayload(payload, file);
  validateReviewNotes(review, file, expectedTarget);
  requireValue(payload.target === expectedTarget, `${file} must target ${expectedTarget}`);
  const refs = payload.conditions?.ref_name?.include ?? [];
  requireValue(refs.length === 1 && refs[0] === expectedRef, `${file} must target ${expectedRef}`);
}

function validateMain(proposal, aggregateName) {
  validateCommon(proposal, "branch", "refs/heads/main");
  const { file, payload } = proposal;
  for (const type of ["deletion", "non_fast_forward", "pull_request", "required_status_checks"]) {
    requireValue(findRule(payload, type), `${file} needs ${type}`);
  }
  const status = findRule(payload, "required_status_checks").parameters;
  const contexts = status.required_status_checks?.map((check) => check.context) ?? [];
  requireValue(contexts.length === 1 && contexts[0] === aggregateName, `${file} must require ${aggregateName}`);
  requireValue(status.strict_required_status_checks_policy === true, `${file} must require current status checks`);
  requireValue(status.do_not_enforce_on_create === false, `${file} must not skip checks on creation`);
  requireValue(findRule(payload, "pull_request").parameters.required_approving_review_count >= 1, `${file} needs a review requirement`);
}

function validateTags(proposal) {
  validateCommon(proposal, "tag", "refs/tags/v*");
  const { file, payload } = proposal;
  for (const type of ["deletion", "non_fast_forward", "update"]) {
    requireValue(findRule(payload, type), `${file} needs ${type}`);
  }
  requireValue(!findRule(payload, "required_signatures"), `${file} must not claim tag-object verification through a ruleset`);
  requireValue(findRule(payload, "update").parameters.update_allows_fetch_and_merge === false, `${file} must restrict all tag updates`);
}

export function validateRuleProposals({ aggregateName, proposals }) {
  const main = proposals.find(({ payload }) => payload.target === "branch");
  const tags = proposals.find(({ payload }) => payload.target === "tag");
  requireValue(main && tags, "proposals must include one branch and one tag payload");
  validateMain(main, aggregateName);
  validateTags(tags);
  return { main, tags };
}

function readProposal(root, definition) {
  const directory = path.join(root, ".github/repository-rules");
  const payloadPath = path.join(directory, definition.payload);
  const reviewPath = path.join(directory, definition.review);
  requireValue(fs.existsSync(payloadPath), `missing proposal ${definition.payload}`);
  requireValue(fs.existsSync(reviewPath), `missing review notes ${definition.review}`);
  try {
    return {
      file: definition.payload,
      payload: JSON.parse(fs.readFileSync(payloadPath, "utf8")),
      review: JSON.parse(fs.readFileSync(reviewPath, "utf8")),
    };
  } catch (error) {
    fail(`invalid JSON for ${definition.payload}: ${error.message}`);
  }
}

export function verifyRepositoryRules(root = ROOT) {
  const workflow = readWorkflowContract(root);
  validateWorkflowContract(workflow);
  const proposals = PROPOSAL_FILES.map((definition) => readProposal(root, definition));
  validateRuleProposals({ aggregateName: workflow.aggregateName, proposals });
  return { ...workflow, proposals };
}

function printUsage() {
  console.log("Usage: node scripts/verify-repository-rules.mjs [--help]");
  console.log("Validates disabled GitHub API payloads and companion review notes locally; it never calls or changes GitHub settings.");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.includes("--help")) printUsage();
    else {
      const result = verifyRepositoryRules();
      console.log(`verify-repository-rules: ${result.proposals.length} disabled API payloads verified with review notes; aggregate ${result.aggregateName}; no GitHub API calls made`);
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
