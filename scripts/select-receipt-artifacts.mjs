#!/usr/bin/env node
// Select the immutable receipt artifacts for one release phase and run attempt.
"use strict";

import path from "node:path";
import { fileURLToPath } from "node:url";
import { runReleaseProcess } from "./release-process.mjs";
import { projectRoot } from "./release-support.mjs";
import {
  assertExactKeys,
  assertPlainObject,
  CHANNELS,
} from "./release-receipt-validation.mjs";

const PHASES = new Set(["exact", "default"]);
const PACKAGES = Object.freeze([...CHANNELS.npmPlatforms]);
const POSITIVE_DECIMAL = /^[1-9][0-9]*$/u;
const MAX_DECIMAL_DIGITS = 128;
const MAX_ARTIFACTS = 1000;
const MAX_PAGES = 1000;
const MAX_METADATA_BYTES = 64 * 1024;
const MAX_NAME_LENGTH = 512;
const SUBPROCESS_TIMEOUT_MS = 60_000;
const MAX_OUTPUT_BYTES = 4 * 1024 * 1024;
const SELECTOR_ERROR = "release receipt artifact selection failed";
const VALUE_OPTIONS = new Set(["--phase", "--attempt", "--run-id", "--repo"]);

function fail(message) {
  throw new Error(`receipt artifact selector: ${message}`);
}

function positiveInteger(value, label) {
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value) || value < 1) fail(`${label} must be positive`);
    return BigInt(value);
  }
  if (typeof value === "bigint") {
    if (value < 1n) fail(`${label} must be positive`);
    return value;
  }
  if (typeof value !== "string" || value.length > MAX_DECIMAL_DIGITS || !POSITIVE_DECIMAL.test(value)) {
    fail(`${label} must be a positive decimal integer`);
  }
  return BigInt(value);
}

function normalizeOptions(options) {
  assertPlainObject(options, "selector options");
  assertExactKeys(options, ["phase", "attempt", "runId"], "selector options");
  if (typeof options.phase !== "string" || !PHASES.has(options.phase)) fail("phase must be exact or default");
  return {
    phase: options.phase,
    attempt: positiveInteger(options.attempt, "attempt"),
    runId: positiveInteger(options.runId, "run ID"),
  };
}

function validateMetadataSize(value, label) {
  let serialized;
  try {
    serialized = JSON.stringify(value);
  } catch {
    fail(`${label} metadata is malformed`);
  }
  if (typeof serialized !== "string" || Buffer.byteLength(serialized, "utf8") > MAX_METADATA_BYTES) {
    fail(`${label} metadata is too large`);
  }
}

function validateWorkflowRun(artifact, label, runId) {
  if (!Object.prototype.hasOwnProperty.call(artifact, "workflow_run")) return;
  const workflowRun = artifact.workflow_run;
  assertPlainObject(workflowRun, `${label}.workflow_run`);
  if (Object.prototype.hasOwnProperty.call(workflowRun, "id")) {
    if (positiveInteger(workflowRun.id, `${label}.workflow_run.id`) !== runId) fail(`${label} belongs to another run`);
  }
}

function validateArtifactMetadata(artifact, index, runId) {
  const label = `artifacts[${index}]`;
  assertPlainObject(artifact, label);
  validateMetadataSize(artifact, label);
  for (const key of ["id", "name", "expired"]) {
    if (!Object.prototype.hasOwnProperty.call(artifact, key)) fail(`${label}.${key} is required`);
  }
  const id = positiveInteger(artifact.id, `${label}.id`);
  if (typeof artifact.name !== "string" || artifact.name.length === 0 || artifact.name.length > MAX_NAME_LENGTH) {
    fail(`${label}.name is invalid`);
  }
  if (typeof artifact.expired !== "boolean") fail(`${label}.expired is invalid`);
  validateWorkflowRun(artifact, label, runId);
  return { id: id.toString(), name: artifact.name, expired: artifact.expired };
}

function selectedCandidate(name, phase) {
  const prefix = `release-receipt-${phase}-`;
  if (!name.startsWith(prefix)) return null;
  const suffix = name.slice(prefix.length);
  for (const packageName of PACKAGES) {
    const marker = `${packageName}-attempt-`;
    if (!suffix.startsWith(marker)) continue;
    const attemptText = suffix.slice(marker.length);
    if (attemptText.length > MAX_DECIMAL_DIGITS || !POSITIVE_DECIMAL.test(attemptText)) {
      fail("selected receipt artifact has a nonpositive attempt");
    }
    return { packageName, attempt: BigInt(attemptText) };
  }
  fail("selected receipt artifact has a malformed name");
}

export function selectReceiptArtifacts(artifacts, options) {
  const { phase, attempt: currentAttempt, runId } = normalizeOptions(options);
  if (!Array.isArray(artifacts) || artifacts.length > MAX_ARTIFACTS) fail("artifact metadata exceeds the limit");
  const latest = new Map();
  const seenIds = new Set();
  const seenPackageAttempts = new Set();
  for (const [index, artifact] of artifacts.entries()) {
    const metadata = validateArtifactMetadata(artifact, index, runId);
    const candidate = selectedCandidate(metadata.name, phase);
    if (!candidate) continue;
    if (candidate.attempt > currentAttempt) fail("selected receipt artifact is from a future attempt");
    if (seenIds.has(metadata.id)) fail("selected receipt artifacts contain a duplicate ID");
    seenIds.add(metadata.id);
    const key = `${candidate.packageName}:${candidate.attempt}`;
    if (seenPackageAttempts.has(key)) fail("selected receipt artifacts contain a duplicate package attempt");
    seenPackageAttempts.add(key);
    const previous = latest.get(candidate.packageName);
    if (!previous || candidate.attempt > previous.attempt) {
      latest.set(candidate.packageName, { ...candidate, id: metadata.id, expired: metadata.expired });
    }
  }
  return PACKAGES.map((packageName) => {
    const selected = latest.get(packageName);
    if (!selected) fail(`missing selected receipt artifact for ${packageName}`);
    if (selected.expired) fail(`latest selected receipt artifact for ${packageName} is expired`);
    return selected.id;
  });
}

function parsePageTotal(value, label) {
  if (value === 0 || value === 0n || value === "0") return 0n;
  try {
    return positiveInteger(value, `${label}.total_count`);
  } catch {
    fail(`${label}.total_count is invalid`);
  }
}

function validatePageTotal(page, label) {
  if (!Object.prototype.hasOwnProperty.call(page, "total_count")) return;
  if (parsePageTotal(page.total_count, label) > BigInt(MAX_ARTIFACTS)) fail("artifact metadata exceeds the limit");
}

function appendPageArtifacts(page, index, artifacts) {
  const label = `page ${index}`;
  assertPlainObject(page, label);
  validateMetadataSize(page, label);
  if (!Array.isArray(page.artifacts)) fail(`${label}.artifacts must be an array`);
  validatePageTotal(page, label);
  if (artifacts.length + page.artifacts.length > MAX_ARTIFACTS) fail("artifact metadata exceeds the limit");
  artifacts.push(...page.artifacts);
}

function parseArtifactPages(output) {
  let pages;
  try {
    pages = JSON.parse(output);
  } catch {
    fail("gh returned malformed JSON");
  }
  if (!Array.isArray(pages) || pages.length > MAX_PAGES) fail("gh returned malformed pagination");
  const artifacts = [];
  for (const [index, page] of pages.entries()) appendPageArtifacts(page, index, artifacts);
  return artifacts;
}

function commandEnvironment(token) {
  const environment = { GH_TOKEN: token, GH_HOST: "github.com" };
  for (const name of ["PATH", "HOME", "TMP", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE", "LC_MESSAGES", "LC_NUMERIC", "LC_TIME", "LC_COLLATE", "LC_MONETARY"]) {
    if (process.env[name]) environment[name] = process.env[name];
  }
  return environment;
}

function validateRepository(value) {
  if (typeof value !== "string" || !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/u.test(value)) fail("repo must be OWNER/REPO");
  return value;
}

function parseArguments(argv) {
  const options = Object.create(null);
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index];
    if (!VALUE_OPTIONS.has(option) || Object.prototype.hasOwnProperty.call(options, option)) fail("invalid command arguments");
    const value = argv[index + 1];
    if (!value || value.startsWith("-")) fail("invalid command arguments");
    options[option] = value;
    index += 1;
  }
  if (Object.keys(options).length !== VALUE_OPTIONS.size) fail("invalid command arguments");
  const normalized = normalizeOptions({ phase: options["--phase"], attempt: options["--attempt"], runId: options["--run-id"] });
  return { ...normalized, repo: validateRepository(options["--repo"]) };
}

async function queryArtifacts(options) {
  if (!process.env.GH_TOKEN) fail("GH_TOKEN is required");
  const endpoint = `repos/${options.repo}/actions/runs/${options.runId.toString()}/artifacts?per_page=100`;
  return runReleaseProcess("gh", ["api", "--paginate", "--slurp", "-X", "GET", endpoint], {
    cwd: projectRoot,
    env: commandEnvironment(process.env.GH_TOKEN),
    timeoutMs: SUBPROCESS_TIMEOUT_MS,
    maxBuffer: MAX_OUTPUT_BYTES,
  });
}

async function main(argv) {
  const options = parseArguments(argv);
  const output = await queryArtifacts(options);
  const ids = selectReceiptArtifacts(parseArtifactPages(output), {
    phase: options.phase,
    attempt: options.attempt,
    runId: options.runId,
  });
  process.stdout.write(`${ids.join(",")}\n`);
}

function reportFailure() {
  process.stderr.write(`${SELECTOR_ERROR}\n`);
  process.exitCode = 1;
}

const invokedDirectly = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) main(process.argv.slice(2)).catch(reportFailure);
