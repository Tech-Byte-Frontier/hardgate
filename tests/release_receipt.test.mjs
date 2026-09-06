// Behavioral contract for the local release receipt/state library.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  CHANNELS,
  MAX_RECEIPT_BYTES,
  RECEIPT_STATES,
  REQUIRED_CHANNELS,
  createReceipt,
  readReceipt,
  readReceiptAsync,
  receiptComplete,
  recordFailure,
  recordTransition,
  validateReceipt,
  writeReceiptAtomic,
  writeReceiptAtomicSync,
} from "../scripts/release-receipt.mjs";

const clone = (value) => JSON.parse(JSON.stringify(value));
const h40 = (letter) => letter.repeat(40);
const h64 = (letter) => letter.repeat(64);

const identity = {
  version: "0.5.0",
  source_sha: h40("a"),
  tooling_sha: h40("b"),
  signed_tag_object: h40("c"),
  build_run_id: "33926961536",
  artifact_id: "987654321",
  archives: [
    { name: "hardgate-linux-x64.tar.gz", sha256: h64("2") },
    { name: "hardgate-wrapper.tgz", sha256: h64("3") },
  ],
};

const evidence = (receipt, hash = h64("9"), includeConsumer = true) => {
  const result = {
    version: receipt.identity.version,
    source_sha: receipt.identity.source_sha,
    archives: clone(receipt.identity.archives),
  };
  if (includeConsumer) result.consumer = { executable: "hardgate", sha256: hash };
  return result;
};

const transitionPairs = RECEIPT_STATES.slice(0, -1).map((from, index) => ({
  from,
  to: RECEIPT_STATES[index + 1],
}));

function advance(receipt, channel, stop = transitionPairs.length) {
  for (const [index, pair] of transitionPairs.entries()) {
    if (index >= stop) break;
    recordTransition(receipt, { channel, ...pair, evidence: evidence(receipt, h64(String(index + 3))) });
  }
  return receipt;
}

function assertReceiptRejects(value, expected = undefined) {
  assert.throws(() => validateReceipt(value, expected), /release receipt:/);
}

assert.deepEqual(CHANNELS.npmPlatforms, [
  "hardgate-linux-x64",
]);
assert.equal(REQUIRED_CHANNELS.length, 4);

const pending = createReceipt(identity, REQUIRED_CHANNELS);
assert.equal(pending.schema_version, 1);
assert.equal(pending.complete, false);
assert.deepEqual(Object.keys(pending.channels), REQUIRED_CHANNELS);
assert.ok(REQUIRED_CHANNELS.every((channel) => pending.channels[channel].state === "pending"));
assert.equal(receiptComplete(pending), false);
validateReceipt(pending, identity);

// Every irreversible step can be checkpointed, reloaded, and resumed. The
// all-channel result stays incomplete until the last required channel finishes.
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-release-receipt-"));
try {
  let receipt = pending;
  for (const [channelIndex, channel] of REQUIRED_CHANNELS.entries()) {
    for (const pair of transitionPairs) {
      recordTransition(receipt, { channel, ...pair, evidence: evidence(receipt, h64(String(channelIndex + 1))) });
      const checkpoint = path.join(directory, `checkpoint-${channelIndex}-${pair.to}.json`);
      await writeReceiptAtomic(checkpoint, receipt, identity);
      receipt = await readReceiptAsync(checkpoint, identity);
      assert.equal(receipt.channels[channel].state, pair.to);
    }
    assert.equal(receiptComplete(receipt), channelIndex === REQUIRED_CHANNELS.length - 1);
  }
  assert.equal(receiptComplete(receipt), true);
  assert.equal(receipt.complete, true);
  assert.deepEqual(readReceipt(path.join(directory, `checkpoint-${REQUIRED_CHANNELS.length - 1}-default_consumer_verified.json`), identity), receipt);
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}

// A failure is append-only and cannot move a channel forward. It can be
// recorded before and between transitions without changing the aggregate.
const failed = createReceipt(identity);
const failureChannel = REQUIRED_CHANNELS[0];
recordFailure(failed, { channel: failureChannel, code: "registry-timeout", message: "registry probe deadline elapsed" });
assert.equal(failed.channels[failureChannel].state, "pending");
assert.equal(failed.channels[failureChannel].events.length, 1);
recordTransition(failed, { channel: failureChannel, from: "pending", to: "staged", evidence: evidence(failed) });
recordFailure(failed, { channel: failureChannel, code: "consumer-retry", message: "consumer retry requested", evidence: evidence(failed) });
assert.equal(failed.channels[failureChannel].state, "staged");
assert.equal(failed.channels[failureChannel].events.length, 3);
assert.equal(receiptComplete(failed), false);

// Artifact staging and immutable verification do not execute a consumer yet,
// so identity/archive evidence is sufficient. Consumer evidence is mandatory
// exactly when entering the two consumer-verification states.
const stageOnly = createReceipt(identity);
recordTransition(stageOnly, { channel: failureChannel, from: "pending", to: "staged", evidence: evidence(stageOnly, h64("6"), false) });
recordTransition(stageOnly, { channel: failureChannel, from: "staged", to: "immutable_verified", evidence: evidence(stageOnly, h64("6"), false) });
assert.throws(() => recordTransition(stageOnly, {
  channel: failureChannel,
  from: "immutable_verified",
  to: "exact_consumer_verified",
  evidence: evidence(stageOnly, h64("6"), false),
}), /consumer is required/);
recordTransition(stageOnly, { channel: failureChannel, from: "immutable_verified", to: "exact_consumer_verified", evidence: evidence(stageOnly, h64("6")) });
recordTransition(stageOnly, { channel: failureChannel, from: "exact_consumer_verified", to: "promoted", evidence: evidence(stageOnly, h64("6"), false) });
assert.throws(() => recordTransition(stageOnly, {
  channel: failureChannel,
  from: "promoted",
  to: "default_consumer_verified",
  evidence: evidence(stageOnly, h64("6"), false),
}), /consumer is required/);
recordTransition(stageOnly, { channel: failureChannel, from: "promoted", to: "default_consumer_verified", evidence: evidence(stageOnly, h64("6")) });

const historicalConsumerEvidence = createReceipt(identity);
recordTransition(historicalConsumerEvidence, { channel: failureChannel, from: "pending", to: "staged", evidence: evidence(historicalConsumerEvidence, h64("7"), false) });
recordTransition(historicalConsumerEvidence, { channel: failureChannel, from: "staged", to: "immutable_verified", evidence: evidence(historicalConsumerEvidence, h64("7"), false) });
recordTransition(historicalConsumerEvidence, { channel: failureChannel, from: "immutable_verified", to: "exact_consumer_verified", evidence: evidence(historicalConsumerEvidence, h64("7")) });
const forgedHistoricalConsumer = clone(historicalConsumerEvidence);
delete forgedHistoricalConsumer.channels[failureChannel].events[2].evidence.consumer;
assertReceiptRejects(forgedHistoricalConsumer);

// Replaying an exact transition is safe and does not add another event. An
// ambiguous retry with changed evidence is rejected instead of advancing twice.
const retried = createReceipt(identity);
const retryOperation = { channel: failureChannel, from: "pending", to: "staged", evidence: evidence(retried, h64("4")) };
recordTransition(retried, retryOperation);
const eventCount = retried.channels[failureChannel].events.length;
recordTransition(retried, clone(retryOperation));
assert.equal(retried.channels[failureChannel].events.length, eventCount);
assert.throws(() => recordTransition(retried, { ...retryOperation, evidence: evidence(retried, h64("5")) }), /replayed transition evidence/);
assert.throws(() => recordTransition(retried, { channel: failureChannel, from: "pending", to: "immutable_verified", evidence: evidence(retried) }), /transition source state/);
assert.throws(() => recordTransition(retried, { channel: failureChannel, from: "immutable_verified", to: "exact_consumer_verified", evidence: evidence(retried) }), /source state/);

// The receipt is strict on import: forged aggregate state, missing channels,
// unknown keys, arbitrary states, and bypassed histories all fail closed.
const malformed = createReceipt(identity);
const forgedComplete = clone(malformed);
forgedComplete.complete = true;
assertReceiptRejects(forgedComplete);
const missingChannel = clone(malformed);
delete missingChannel.channels[REQUIRED_CHANNELS.at(-1)];
assertReceiptRejects(missingChannel);
const unknownKey = clone(malformed);
unknownKey.untrusted = true;
assertReceiptRejects(unknownKey);
const arbitraryState = clone(malformed);
arbitraryState.channels[failureChannel].state = "published";
assertReceiptRejects(arbitraryState);
const bypassedHistory = clone(malformed);
bypassedHistory.channels[failureChannel].state = "exact_consumer_verified";
bypassedHistory.channels[failureChannel].events.push({
  type: "transition",
  from: "pending",
  to: "exact_consumer_verified",
  evidence: evidence(malformed),
});
assertReceiptRejects(bypassedHistory);
const unknownEventKey = clone(malformed);
unknownEventKey.channels[failureChannel].events.push({
  type: "transition",
  from: "pending",
  to: "staged",
  evidence: evidence(malformed),
  verifier: "untrusted",
});
unknownEventKey.channels[failureChannel].state = "staged";
assertReceiptRejects(unknownEventKey);

// Recovery requires every identity field and every archive digest to match;
// source and tooling identities are independently persisted and can differ.
const recoveryReceipt = createReceipt(identity);
const identityFields = ["version", "source_sha", "tooling_sha", "signed_tag_object", "build_run_id", "artifact_id"];
for (const field of identityFields) {
  const wrong = clone(identity);
  if (field === "version") wrong[field] = "0.5.1";
  else if (field === "build_run_id" || field === "artifact_id") wrong[field] = "999999999";
  else wrong[field] = field === "tooling_sha" ? h40("e") : h40("f");
  assert.throws(() => validateReceipt(recoveryReceipt, wrong), /identity/);
}
const wrongDigest = clone(identity);
wrongDigest.archives[0].sha256 = h64("a");
assert.throws(() => validateReceipt(recoveryReceipt, wrongDigest), /identity/);
assert.notEqual(identity.source_sha, identity.tooling_sha, "recovery identity must preserve distinct source/tooling SHAs");

const wrongEvidenceVersion = evidence(recoveryReceipt);
wrongEvidenceVersion.version = "0.5.1";
assert.throws(() => recordTransition(recoveryReceipt, {
  channel: failureChannel,
  from: "pending",
  to: "staged",
  evidence: wrongEvidenceVersion,
}), /evidence/);
const wrongEvidenceDigest = evidence(recoveryReceipt);
wrongEvidenceDigest.archives[0].sha256 = h64("a");
assert.throws(() => recordTransition(recoveryReceipt, {
  channel: failureChannel,
  from: "pending",
  to: "staged",
  evidence: wrongEvidenceDigest,
}), /archives/);

for (const name of ["../escape.tar.gz", "foo/bar.tar.gz", "foo\\bar.tar.gz", "foo/../../bar.tar.gz"]) {
  const badName = clone(identity);
  badName.archives[0].name = name;
  assert.throws(() => createReceipt(badName), /archive name/);
}

// Proposed event validation is transactional: a failed append at the event
// limit leaves the caller's mutable receipt byte-for-byte unchanged.
function receiptAtEventLimit() {
  const result = createReceipt(identity);
  result.channels[failureChannel].events = Array.from({ length: 4096 }, () => ({
    type: "failure",
    state: "pending",
    code: "retry",
    message: "retry recorded",
  }));
  validateReceipt(result);
  return result;
}

const transitionOverflow = receiptAtEventLimit();
const transitionBefore = clone(transitionOverflow);
assert.throws(() => recordTransition(transitionOverflow, {
  channel: failureChannel,
  from: "pending",
  to: "staged",
  evidence: evidence(transitionOverflow, h64("8"), false),
}), /4096/);
assert.deepEqual(transitionOverflow, transitionBefore);

const failureOverflow = receiptAtEventLimit();
const failureBefore = clone(failureOverflow);
assert.throws(() => recordFailure(failureOverflow, {
  channel: failureChannel,
  code: "retry-overflow",
  message: "retry recorded",
}), /4096/);
assert.deepEqual(failureOverflow, failureBefore);

// IO stays JSON-only and refuses symlink targets. The synchronous writer is
// covered as well because recovery tooling may run outside an async workflow.
const ioDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-release-receipt-io-"));
try {
  const receiptPath = path.join(ioDirectory, "receipt.json");
  writeReceiptAtomicSync(receiptPath, pending, identity);
  assert.deepEqual(readReceipt(receiptPath, identity), pending);
  const asyncPath = path.join(ioDirectory, "async-receipt.json");
  await writeReceiptAtomic(asyncPath, pending, identity);
  assert.deepEqual(await readReceiptAsync(asyncPath, identity), pending);
  const malformedPath = path.join(ioDirectory, "malformed.json");
  fs.writeFileSync(malformedPath, "{ definitely not json\n");
  assert.throws(() => readReceipt(malformedPath), /valid JSON/);
  const oversizedPath = path.join(ioDirectory, "oversized.json");
  fs.writeFileSync(oversizedPath, Buffer.alloc(MAX_RECEIPT_BYTES + 1, 0x20));
  assert.throws(() => readReceipt(oversizedPath), /size limit/);
  await assert.rejects(readReceiptAsync(oversizedPath), /size limit/);
  const symlinkPath = path.join(ioDirectory, "receipt-link.json");
  fs.symlinkSync(receiptPath, symlinkPath);
  await assert.rejects(writeReceiptAtomic(symlinkPath, pending, identity), /symbolic-link/);
  assert.throws(() => readReceipt(symlinkPath), /symbolic-link/);
  assert.equal(fs.readFileSync(receiptPath, "utf8"), fs.readFileSync(asyncPath, "utf8"));
} finally {
  fs.rmSync(ioDirectory, { recursive: true, force: true });
}

// Secret-shaped fields and values never enter a persisted receipt.
const secretField = clone(pending);
secretField.token = "must-not-persist";
assertReceiptRejects(secretField);
assert.throws(() => recordFailure(createReceipt(identity), {
  channel: failureChannel,
  code: "bad-input",
  message: "authorization: bearer abc",
}), /secret/);

// A receipt missing only one terminal channel is still incomplete.
const almostComplete = createReceipt(identity);
for (const channel of REQUIRED_CHANNELS.slice(0, -1)) advance(almostComplete, channel);
assert.equal(receiptComplete(almostComplete), false);
assert.equal(almostComplete.complete, false);
validateReceipt(almostComplete, identity);

console.log("release_receipt.test: OK (schema, staged transitions, retry, failure, recovery, atomic IO)");
