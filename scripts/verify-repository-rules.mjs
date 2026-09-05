import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PROPOSAL_FILES = [
  "main-branch.review-only.json",
  "version-tags.review-only.json",
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

export function readWorkflowContract(root = ROOT) {
  const workflow = fs.readFileSync(path.join(root, ".github/workflows/ci.yml"), "utf8");
  const quality = jobBlock(workflow, "quality");
  return { aggregateName: scalar(quality, "name"), needs: inlineList(quality, "needs") };
}

function findRule(proposal, type) {
  return proposal.rules.find((rule) => rule.type === type);
}

function validateCommon(proposal, expectedTarget, expectedRef) {
  requireValue(proposal.target === expectedTarget, `${proposal.name} must target ${expectedTarget}`);
  requireValue(proposal.enforcement === "disabled", `${proposal.name} must remain disabled`);
  requireValue(proposal.bypass_actors?.length === 0, `${proposal.name} must not invent bypass actors`);
  requireValue(proposal._proposal?.status === "REVIEW-ONLY", `${proposal.name} must be review-only`);
  requireValue(proposal._proposal?.recovery?.maintainer === "repository administrator", `${proposal.name} needs an explicit recovery maintainer`);
  requireValue(proposal._proposal.recovery.steps?.length >= 2, `${proposal.name} needs recovery steps`);
  const refs = proposal.conditions?.ref_name?.include ?? [];
  requireValue(refs.length === 1 && refs[0] === expectedRef, `${proposal.name} must target ${expectedRef}`);
  requireValue(Array.isArray(proposal.rules), `${proposal.name} must declare rules`);
}

function validateMain(proposal, aggregateName) {
  validateCommon(proposal, "branch", "refs/heads/main");
  for (const type of ["deletion", "non_fast_forward", "pull_request", "required_status_checks"]) {
    requireValue(findRule(proposal, type), `${proposal.name} needs ${type}`);
  }
  const status = findRule(proposal, "required_status_checks").parameters;
  const contexts = status.required_status_checks?.map((check) => check.context) ?? [];
  requireValue(contexts.length === 1 && contexts[0] === aggregateName, `${proposal.name} must require ${aggregateName}`);
  requireValue(status.strict_required_status_checks_policy === true, `${proposal.name} must require current status checks`);
  requireValue(status.do_not_enforce_on_create === false, `${proposal.name} must not skip checks on creation`);
  requireValue(findRule(proposal, "pull_request").parameters.required_approving_review_count >= 1, `${proposal.name} needs a review requirement`);
}

function validateTags(proposal) {
  validateCommon(proposal, "tag", "refs/tags/v*");
  for (const type of ["deletion", "non_fast_forward", "required_signatures"]) {
    requireValue(findRule(proposal, type), `${proposal.name} needs ${type}`);
  }
}

export function verifyRepositoryRules(root = ROOT) {
  const workflow = readWorkflowContract(root);
  const proposals = PROPOSAL_FILES.map((file) => {
    const fullPath = path.join(root, ".github/repository-rules", file);
    requireValue(fs.existsSync(fullPath), `missing proposal ${file}`);
    try {
      return { file, value: JSON.parse(fs.readFileSync(fullPath, "utf8")) };
    } catch (error) {
      fail(`invalid JSON in ${file}: ${error.message}`);
    }
  });
  const main = proposals.find(({ value }) => value.target === "branch");
  const tags = proposals.find(({ value }) => value.target === "tag");
  requireValue(main && tags, "proposals must include one branch and one tag payload");
  validateMain(main.value, workflow.aggregateName);
  validateTags(tags.value);
  return { ...workflow, proposals };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const result = verifyRepositoryRules();
    console.log(`verify-repository-rules: ${result.proposals.length} disabled proposals require ${result.aggregateName}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
