// Behavioral contract for strict, prefix-only release receipt aggregation.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import {
  REQUIRED_CHANNELS,
  createReceipt,
  readReceipt,
  recordFailure,
  recordTransition,
  writeReceiptAtomicSync,
} from "../scripts/release-receipt.mjs";
import { mergeReceipts } from "../scripts/merge-release-receipts.mjs";
import { projectRoot } from "../scripts/release-support.mjs";

const h40 = (character) => character.repeat(40);
const h64 = (character) => character.repeat(64);
const copy = (value) => JSON.parse(JSON.stringify(value));
const archiveNames = "hardgate-linux-x64.tar.gz hardgate-wrapper.tgz".split(" ");
const identity = {
  version: "0.5.0",
  source_sha: h40("a"), tooling_sha: h40("b"), signed_tag_object: h40("c"),
  build_run_id: "33926961536", artifact_id: "987654321",
  archives: archiveNames.map((name, index) => ({ name, sha256: h64(String(index)) })),
};
const transitions = [
  ["pending", "staged"],
  ["staged", "immutable_verified"],
  ["immutable_verified", "exact_consumer_verified"],
  ["exact_consumer_verified", "promoted"],
  ["promoted", "default_consumer_verified"],
];

function evidence(receipt, consumerHash = h64("9")) {
  const result = {
    version: receipt.identity.version,
    source_sha: receipt.identity.source_sha,
    archives: copy(receipt.identity.archives),
  };
  if (consumerHash !== null) result.consumer = { executable: "hardgate", sha256: consumerHash };
  return result;
}

function channelReceipt(channel, stop = transitions.length, consumerHash = h64("9")) {
  const receipt = createReceipt(identity);
  for (const [index, [from, to]] of transitions.entries()) {
    if (index >= stop) break;
    recordTransition(receipt, { channel, from, to, evidence: evidence(receipt, consumerHash) });
  }
  return receipt;
}

function runCli(argumentsList) {
  return spawnSync(process.execPath, [path.join(projectRoot, "scripts/merge-release-receipts.mjs"), ...argumentsList], {
    cwd: projectRoot,
    encoding: "utf8",
  });
}

function expectReject(action, pattern = /release receipt:/) {
  assert.throws(action, pattern);
}

const fragments = REQUIRED_CHANNELS.map((channel, index) => channelReceipt(channel, transitions.length, h64(String(index + 1))));
const fragmentSnapshots = copy(fragments);
const complete = mergeReceipts([...fragments].reverse(), identity);
assert.equal(complete.complete, true);
assert.deepEqual(complete.channels, Object.fromEntries(REQUIRED_CHANNELS.map((channel) => [channel, fragments.find((item) => item.channels[channel].state === "default_consumer_verified").channels[channel]])));
assert.deepEqual(fragments, fragmentSnapshots, "aggregation must not mutate fragments");

const prefix = channelReceipt(REQUIRED_CHANNELS[0], 2);
const extension = channelReceipt(REQUIRED_CHANNELS[0], 4);
const mergedPrefix = mergeReceipts([prefix, extension, copy(extension)], identity);
assert.equal(mergedPrefix.channels[REQUIRED_CHANNELS[0]].state, "promoted");
assert.equal(mergedPrefix.channels[REQUIRED_CHANNELS[1]].state, "pending");
expectReject(() => mergeReceipts([channelReceipt(REQUIRED_CHANNELS[0], 4, h64("a")), channelReceipt(REQUIRED_CHANNELS[0], 4, h64("b"))], identity), /divergent/);

const failedA = createReceipt(identity);
const failedB = createReceipt(identity);
recordFailure(failedA, { channel: REQUIRED_CHANNELS[0], code: "retry-a", message: "first retry" });
recordFailure(failedB, { channel: REQUIRED_CHANNELS[0], code: "retry-b", message: "second retry" });
expectReject(() => mergeReceipts([failedA, failedB], identity), /divergent/);

for (const field of ["version", "source_sha", "tooling_sha", "signed_tag_object", "build_run_id", "artifact_id"]) {
  const wrong = copy(identity);
  wrong[field] = ["build_run_id", "artifact_id"].includes(field) ? "123456789" : field === "version" ? "0.5.1" : h40("e");
  expectReject(() => mergeReceipts([createReceipt(wrong)], identity), /identity/);
}
const wrongArchive = copy(identity);
wrongArchive.archives[0].sha256 = h64("e");
expectReject(() => mergeReceipts([createReceipt(wrongArchive)], identity), /identity/);
expectReject(() => mergeReceipts([], identity), /non-empty/);
const forgedComplete = createReceipt(identity);
forgedComplete.complete = true;
expectReject(() => mergeReceipts([forgedComplete], identity), /complete/);
const unknownKey = createReceipt(identity);
unknownKey.untrusted = true;
expectReject(() => mergeReceipts([unknownKey], identity), /unknown keys/);

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-receipt-aggregation-"));
try {
  const expectedPath = path.join(directory, "expected.json");
  const inputPath = path.join(directory, "input.json");
  const outputPath = path.join(directory, "merged-incomplete.json");
  writeReceiptAtomicSync(expectedPath, createReceipt(identity), identity);
  writeReceiptAtomicSync(inputPath, channelReceipt(REQUIRED_CHANNELS[0], 1), identity);

  const incomplete = runCli(["--expected", expectedPath, "--output", outputPath, "--require-complete", inputPath]);
  assert.notEqual(incomplete.status, 0, "--require-complete must reject an incomplete aggregate");
  assert.match(incomplete.stdout, /0\/7 channels complete \(incomplete\)/);
  assert.doesNotMatch(incomplete.stdout, new RegExp(identity.source_sha));
  assert.equal(readReceipt(outputPath, identity).complete, false, "incomplete output must be persisted for recovery");

  const completeInputs = [];
  for (const [index, fragment] of fragments.entries()) {
    const input = path.join(directory, `complete-${index}.json`);
    writeReceiptAtomicSync(input, fragment, identity);
    completeInputs.push(input);
  }
  const completeOutput = path.join(directory, "merged-complete.json");
  const completed = runCli(["--expected", expectedPath, "--output", completeOutput, "--require-complete", ...completeInputs]);
  assert.equal(completed.status, 0, completed.stderr);
  assert.match(completed.stdout, /7\/7 channels complete \(complete\)/);
  assert.doesNotMatch(completed.stdout, new RegExp(identity.source_sha));
  assert.equal(readReceipt(completeOutput, identity).complete, true);

  const malformed = path.join(directory, "malformed.json");
  fs.writeFileSync(malformed, "{not-json\n");
  assert.notEqual(runCli(["--expected", expectedPath, "--output", outputPath, malformed]).status, 0);
  assert.notEqual(runCli(["--expected", expectedPath, "--output", outputPath, "--unknown", inputPath]).status, 0);
  assert.notEqual(runCli(["--expected", "--output", outputPath, inputPath]).status, 0);
  assert.notEqual(runCli(["--expected", expectedPath, "--output", outputPath]).status, 0);

  const expectedLink = path.join(directory, "expected-link.json");
  fs.symlinkSync(expectedPath, expectedLink);
  assert.notEqual(runCli(["--expected", expectedLink, "--output", outputPath, inputPath]).status, 0);
  const inputLink = path.join(directory, "input-link.json");
  fs.symlinkSync(inputPath, inputLink);
  assert.notEqual(runCli(["--expected", expectedPath, "--output", outputPath, inputLink]).status, 0);
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}

console.log("release_receipt_aggregation.test: OK (validated prefix merge, identity, CLI recovery and completion)");
