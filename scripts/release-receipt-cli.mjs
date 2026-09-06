#!/usr/bin/env node
// Local release-receipt checkpoint CLI. It records caller-supplied evidence;
// it cannot independently prove a remote registry or GitHub artifact state.
"use strict";

import { PLATFORM_ASSETS as RELEASE_ASSETS } from "./release-platforms.mjs";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  REQUIRED_CHANNELS,
  RECEIPT_STATES,
  assertChannelName,
  clone,
  fail,
  validateIdentity,
} from "./release-receipt-validation.mjs";
import {
  createReceipt,
  readReceipt,
  recordFailure,
  recordTransition,
  writeReceiptAtomicSync,
} from "./release-receipt.mjs";

const PLATFORM_ASSETS = RELEASE_ASSETS;
const HASH_CHUNK_BYTES = 64 * 1024;
const MAX_HASH_BYTES = 1024 * 1024 * 1024;
const READ_FLAGS = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0);
const ZERO_HASH = "0".repeat(64);
const CONSUMER_STATES = new Set(["exact_consumer_verified", "default_consumer_verified"]);
const VALUE_OPTIONS = new Set([
  "--output",
  "--version",
  "--source-sha",
  "--tooling-sha",
  "--tag-object",
  "--build-run-id",
  "--artifact-id",
  "--dist",
  "--receipt",
  "--channel",
  "--to",
  "--consumer",
  "--code",
  "--message",
]);
const COMMAND_OPTIONS = Object.freeze({
  create: new Set([
    "--output",
    "--version",
    "--source-sha",
    "--tooling-sha",
    "--tag-object",
    "--build-run-id",
    "--artifact-id",
    "--dist",
  ]),
  advance: new Set(["--receipt", "--channel", "--to", "--consumer"]),
  failure: new Set(["--receipt", "--channel", "--code", "--message"]),
  assert: new Set(["--receipt", "--require-complete"]),
});

function required(options, names) {
  for (const name of names) if (!options[name]) fail(`${name} is required`);
}

function parseOptions(command, argv) {
  const allowed = COMMAND_OPTIONS[command];
  const options = Object.create(null);
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!allowed.has(argument)) fail(`unknown option ${argument}`);
    if (Object.hasOwn(options, argument)) fail(`${argument} was specified more than once`);
    if (!VALUE_OPTIONS.has(argument)) {
      options[argument] = true;
      continue;
    }
    const value = argv[index + 1];
    if (!value || value.startsWith("-")) fail(`${argument} requires a value`);
    options[argument] = value;
    index += 1;
  }
  return options;
}

function expectedAssets(version) {
  return [...PLATFORM_ASSETS, "SHA256SUMS", `hardgate-${version}.sbom.cdx.json`].sort();
}

function validateCreateIdentity(options) {
  return validateIdentity({
    version: options["--version"],
    source_sha: options["--source-sha"],
    tooling_sha: options["--tooling-sha"],
    signed_tag_object: options["--tag-object"],
    build_run_id: options["--build-run-id"],
    artifact_id: options["--artifact-id"],
    archives: [{ name: "identity-placeholder", sha256: ZERO_HASH }],
  });
}

function directoryPath(input) {
  const target = path.resolve(input);
  let stats;
  try {
    stats = fs.lstatSync(target);
  } catch {
    fail("dist must be an existing regular directory");
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) fail("dist must be an existing regular directory");
  return target;
}

function assertRegularInput(file, label) {
  let listed;
  try {
    listed = fs.lstatSync(file);
  } catch {
    fail(`${label} is missing`);
  }
  if (listed.isSymbolicLink()) fail(`${label} must not be a symbolic link`);
  if (!listed.isFile()) fail(`${label} must be a regular file`);
}

function hashDescriptor(descriptor, label) {
  const opened = fs.fstatSync(descriptor);
  if (!opened.isFile()) fail(`${label} must be a regular file`);
  if (!Number.isSafeInteger(opened.size) || opened.size > MAX_HASH_BYTES) {
    fail(`${label} exceeds the bounded hash size`);
  }
  const digest = crypto.createHash("sha256");
  const buffer = Buffer.allocUnsafe(HASH_CHUNK_BYTES);
  let total = 0;
  while (true) {
    const bytesRead = fs.readSync(descriptor, buffer, 0, buffer.length, null);
    if (bytesRead === 0) break;
    total += bytesRead;
    if (total > MAX_HASH_BYTES) fail(`${label} exceeds the bounded hash size`);
    digest.update(buffer.subarray(0, bytesRead));
  }
  return digest.digest("hex");
}

function hashRegularFile(file, label) {
  assertRegularInput(file, label);
  let descriptor;
  try {
    descriptor = fs.openSync(file, READ_FLAGS);
    return hashDescriptor(descriptor, label);
  } catch (error) {
    if (error instanceof Error && error.message.startsWith("release receipt:")) throw error;
    fail(`${label} could not be hashed`);
  } finally {
    if (descriptor !== undefined) fs.closeSync(descriptor);
  }
}

function reportCommandError(error) {
  const message = error instanceof Error ? error.message : "release receipt: command failed";
  process.stderr.write(message);
  process.stderr.write("\n");
  process.exitCode = 1;
}

function collectArchives(dist, names) {
  let entries;
  try {
    entries = fs.readdirSync(dist, { withFileTypes: true }).map((entry) => entry.name);
  } catch {
    fail("dist could not be listed");
  }
  const expected = new Set(names);
  for (const name of names) if (!entries.includes(name)) fail(`missing release asset ${name}`);
  for (const name of entries) if (!expected.has(name)) fail(`unexpected release asset ${name}`);
  return names.map((name) => ({ name, sha256: hashRegularFile(path.join(dist, name), name) }));
}

function identityFromOptions(options, archives) {
  return validateIdentity({
    version: options["--version"],
    source_sha: options["--source-sha"],
    tooling_sha: options["--tooling-sha"],
    signed_tag_object: options["--tag-object"],
    build_run_id: options["--build-run-id"],
    artifact_id: options["--artifact-id"],
    archives,
  });
}

function countStates(receipt) {
  return Object.fromEntries(RECEIPT_STATES.map((state) => [
    state,
    REQUIRED_CHANNELS.filter((channel) => receipt.channels[channel].state === state).length,
  ]));
}

function printStatus(receipt, prefix = "") {
  const counts = countStates(receipt);
  const complete = REQUIRED_CHANNELS.filter((channel) => receipt.channels[channel].state === RECEIPT_STATES.at(-1)).length;
  const status = receipt.complete ? "complete" : "incomplete";
  const states = RECEIPT_STATES.map((state) => `${state}=${counts[state]}`).join(" ");
  console.log(`${prefix}release receipt: ${complete}/${REQUIRED_CHANNELS.length} channels complete (${status}); ${states}`);
}

function createCommand(options) {
  required(options, ["--output", "--version", "--source-sha", "--tooling-sha", "--tag-object", "--build-run-id", "--artifact-id", "--dist"]);
  const checked = validateCreateIdentity(options);
  const dist = directoryPath(options["--dist"]);
  const names = expectedAssets(checked.version);
  const output = path.resolve(options["--output"]);
  if (names.some((name) => path.resolve(dist, name) === output)) fail("receipt output cannot overwrite a release asset");
  const identity = identityFromOptions(options, collectArchives(dist, names));

  let existing;
  try {
    existing = fs.lstatSync(output);
  } catch (error) {
    if (error.code !== "ENOENT") fail("receipt output cannot be inspected");
  }
  if (existing) {
    const receipt = readReceipt(output, identity);
    printStatus(receipt, "preserved: ");
    return;
  }
  const receipt = createReceipt(identity);
  writeReceiptAtomicSync(output, receipt, identity);
  printStatus(receipt, "created: ");
}

function evidenceFor(receipt, consumer) {
  const evidence = {
    version: receipt.identity.version,
    source_sha: receipt.identity.source_sha,
    archives: clone(receipt.identity.archives),
  };
  if (consumer) evidence.consumer = consumer;
  return evidence;
}

function consumerEvidence(input) {
  let resolved;
  try {
    resolved = fs.realpathSync(path.resolve(input));
  } catch {
    fail("consumer must resolve to a regular file");
  }
  return { executable: path.normalize(resolved), sha256: hashRegularFile(resolved, "consumer") };
}

function transitionPlan(receipt, channel, target) {
  assertChannelName(channel);
  if (!RECEIPT_STATES.includes(target)) fail("unknown target state");
  const current = receipt.channels[channel].state;
  const currentIndex = RECEIPT_STATES.indexOf(current);
  const targetIndex = RECEIPT_STATES.indexOf(target);
  if (targetIndex === currentIndex + 1) return { from: current, replay: false };
  if (targetIndex === currentIndex && currentIndex > 0) {
    const event = [...receipt.channels[channel].events].reverse().find((item) => item.type === "transition" && item.to === target);
    if (event) return { from: event.from, replay: true };
  }
  fail("transition must advance exactly one state or replay the current target");
}

function advanceCommand(options) {
  required(options, ["--receipt", "--channel", "--to"]);
  const receipt = readReceipt(options["--receipt"]);
  const channel = options["--channel"];
  const target = options["--to"];
  const plan = transitionPlan(receipt, channel, target);
  const hasConsumer = options["--consumer"] !== undefined;
  if (CONSUMER_STATES.has(target) && !hasConsumer) fail("--consumer is required for a consumer verification state");
  if (!CONSUMER_STATES.has(target) && hasConsumer) fail("--consumer is only valid for a consumer verification state");
  const consumer = hasConsumer ? consumerEvidence(options["--consumer"]) : undefined;
  const evidence = evidenceFor(receipt, consumer);
  recordTransition(receipt, { channel, from: plan.from, to: target, evidence });
  if (!plan.replay) writeReceiptAtomicSync(options["--receipt"], receipt, receipt.identity);
  printStatus(receipt, plan.replay ? "replayed: " : "advanced: ");
}

function failureCommand(options) {
  required(options, ["--receipt", "--channel", "--code", "--message"]);
  const receipt = readReceipt(options["--receipt"]);
  recordFailure(receipt, {
    channel: options["--channel"],
    code: options["--code"],
    message: options["--message"],
  });
  writeReceiptAtomicSync(options["--receipt"], receipt, receipt.identity);
  printStatus(receipt, "failure: ");
}

function assertCommand(options) {
  required(options, ["--receipt"]);
  const receipt = readReceipt(options["--receipt"]);
  printStatus(receipt);
  if (options["--require-complete"] && !receipt.complete) process.exitCode = 1;
}

function run(argv) {
  const command = argv[0];
  if (!command || !Object.hasOwn(COMMAND_OPTIONS, command)) fail("command must be create, advance, failure, or assert");
  const options = parseOptions(command, argv.slice(1));
  if (command === "create") return createCommand(options);
  if (command === "advance") return advanceCommand(options);
  if (command === "failure") return failureCommand(options);
  return assertCommand(options);
}

function main() {
  try {
    run(process.argv.slice(2));
  } catch (error) {
    reportCommandError(error);
  }
}

const invokedDirectly = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) main();
