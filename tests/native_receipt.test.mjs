// Behavioral contract for native worker proof application.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import {
  CHANNELS,
  createReceipt,
  readReceipt,
  recordTransition,
  writeReceiptAtomicSync,
} from "../scripts/release-receipt.mjs";
import { applyNativeProof } from "../scripts/apply-native-receipt.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const node = "/home/tauan/.nvm/versions/node/v26.8.1/bin/node";
const cli = path.join(root, "scripts", "apply-native-receipt.mjs");
const version = "0.5.0";
const packageNames = [...CHANNELS.npmPlatforms];
const archiveNames = [...packageNames.map((name) => `${name}.tar.gz`), "SHA256SUMS", `hardgate-${version}.sbom.cdx.json`].sort();
const h40 = (character) => character.repeat(40);
const h64 = (character) => character.repeat(64);
const copy = (value) => JSON.parse(JSON.stringify(value));
const identity = {
  version,
  source_sha: h40("a"),
  tooling_sha: h40("b"),
  signed_tag_object: h40("c"),
  build_run_id: "33926961536",
  artifact_id: "987654321",
  archives: archiveNames.map((name, index) => ({ name, sha256: h64(String(index)) })),
};

function evidence(receipt, consumer = undefined) {
  const value = {
    version: receipt.identity.version,
    source_sha: receipt.identity.source_sha,
    archives: copy(receipt.identity.archives),
  };
  if (consumer) value.consumer = copy(consumer);
  return value;
}

function advancePrefix(receipt, channel, target) {
  const pairs = [
    ["pending", "staged"],
    ["staged", "immutable_verified"],
    ["immutable_verified", "exact_consumer_verified"],
    ["exact_consumer_verified", "promoted"],
    ["promoted", "default_consumer_verified"],
  ];
  const consumer = { executable: `node_modules/${channel}/bin/hardgate`, sha256: h64("d") };
  for (const [from, to] of pairs) {
    if (to === "exact_consumer_verified" || to === "default_consumer_verified") {
      recordTransition(receipt, { channel, from, to, evidence: evidence(receipt, consumer) });
    } else {
      recordTransition(receipt, { channel, from, to, evidence: evidence(receipt) });
    }
    if (to === target) return receipt;
  }
  assert.fail(`unsupported setup target ${target}`);
}

function receiptAt(channels, target) {
  const receipt = createReceipt(identity);
  for (const channel of channels) advancePrefix(receipt, channel, target);
  return receipt;
}

function proofFor(packageName, mode = "exact") {
  const archiveName = `${packageName}.tar.gz`;
  const archive = identity.archives.find((entry) => entry.name === archiveName);
  const consumer = { executable: `node_modules/${packageName}/bin/hardgate`, sha256: h64("f") };
  const proof = {
    schema_version: 1,
    version,
    source_sha: identity.source_sha,
    mode,
    package: packageName,
    archive: { name: archiveName, sha256: archive.sha256 },
    consumer,
  };
  if (packageName === "hardgate-linux-x64") proof.wrapper = copy(consumer);
  return proof;
}

function expectReject(action, pattern = /release receipt:/) {
  assert.throws(action, pattern);
}

function runCli(args, environment = {}) {
  const result = spawnSync(node, [cli, ...args], {
    cwd: root,
    encoding: "utf8",
    env: { ...process.env, ...environment },
  });
  assert.equal(result.error, undefined, result.error?.message);
  return result;
}

function writeProof(file, proof) {
  fs.writeFileSync(file, `${JSON.stringify(proof, null, 2)}\n`);
}

const linuxPackage = "hardgate-linux-x64";
const exactChannels = [linuxPackage, CHANNELS.npmWrapper];
const exactReceipt = receiptAt(exactChannels, "immutable_verified");
const exactProof = proofFor(linuxPackage);
const exactBefore = copy(exactReceipt);
const proofBefore = copy(exactProof);
const exactApplied = applyNativeProof(exactReceipt, exactProof);
assert.notStrictEqual(exactApplied, exactReceipt);
assert.deepEqual(exactReceipt, exactBefore, "pure application must not mutate the receipt");
assert.deepEqual(exactProof, proofBefore, "pure application must not mutate the proof");
assert.equal(exactApplied.channels[linuxPackage].state, "exact_consumer_verified");
assert.equal(exactApplied.channels[CHANNELS.npmWrapper].state, "exact_consumer_verified");
assert.deepEqual(
  exactApplied.channels[linuxPackage].events.at(-1).evidence.consumer,
  exactApplied.channels[CHANNELS.npmWrapper].events.at(-1).evidence.consumer,
  "wrapper evidence must be identical to the native proof",
);
assert.equal(exactApplied.channels[CHANNELS.crate].state, "pending");
assert.equal(exactApplied.channels[CHANNELS.githubAssets].state, "pending");

const defaultReceipt = receiptAt(exactChannels, "promoted");
const defaultApplied = applyNativeProof(defaultReceipt, proofFor(linuxPackage, "default"));
assert.equal(defaultApplied.channels[linuxPackage].state, "default_consumer_verified");
assert.equal(defaultApplied.channels[CHANNELS.npmWrapper].state, "default_consumer_verified");

const invalidProofs = [];
const wrongVersion = copy(exactProof);
wrongVersion.version = "0.5.1";
invalidProofs.push(wrongVersion);
const wrongSource = copy(exactProof);
wrongSource.source_sha = h40("b");
invalidProofs.push(wrongSource);
const wrongArchiveName = copy(exactProof);
wrongArchiveName.archive.name = "hardgate-darwin-x64.tar.gz";
invalidProofs.push(wrongArchiveName);
const wrongArchiveHash = copy(exactProof);
wrongArchiveHash.archive.sha256 = h64("b");
invalidProofs.push(wrongArchiveHash);
const wrongPackage = copy(exactProof);
wrongPackage.package = "hardgate-darwin-x64";
invalidProofs.push(wrongPackage);
const wrongConsumerPath = copy(exactProof);
wrongConsumerPath.consumer.executable = "node_modules/hardgate-linux-x64/bin/other";
invalidProofs.push(wrongConsumerPath);
const wrongWrapperPath = copy(exactProof);
wrongWrapperPath.wrapper.executable = "node_modules/hardgate/bin/hardgate";
invalidProofs.push(wrongWrapperPath);
const wrongConsumerHash = copy(exactProof);
wrongConsumerHash.consumer.sha256 = h64("a");
wrongConsumerHash.wrapper.sha256 = h64("a");
const wrongSchema = copy(exactProof);
wrongSchema.schema_version = 2;
invalidProofs.push(wrongSchema);
const extraProofField = copy(exactProof);
extraProofField.untrusted = true;
invalidProofs.push(extraProofField);
const extraArchiveField = copy(exactProof);
extraArchiveField.archive.extra = true;
invalidProofs.push(extraArchiveField);
const missingWrapper = copy(exactProof);
delete missingWrapper.wrapper;
invalidProofs.push(missingWrapper);
const otherWrapper = proofFor("hardgate-darwin-x64");
otherWrapper.wrapper = copy(otherWrapper.consumer);
invalidProofs.push(otherWrapper);
for (const proof of invalidProofs) expectReject(() => applyNativeProof(exactReceipt, proof));

expectReject(() => applyNativeProof(createReceipt(identity), proofFor(linuxPackage)));
expectReject(() => applyNativeProof(receiptAt(exactChannels, "promoted"), proofFor(linuxPackage)));
const replayed = applyNativeProof(exactApplied, exactProof);
assert.deepEqual(replayed, exactApplied, "an exact proof replay must be idempotent");
expectReject(() => applyNativeProof(exactApplied, wrongConsumerHash), /replayed transition evidence/);
const changedReplay = copy(exactProof);
changedReplay.consumer.sha256 = h64("a");
changedReplay.wrapper.sha256 = h64("a");
expectReject(() => applyNativeProof(exactApplied, changedReplay), /replayed transition evidence/);

const nonCanonical = receiptAt(["hardgate-darwin-x64"], "immutable_verified");
const nonCanonicalApplied = applyNativeProof(nonCanonical, proofFor("hardgate-darwin-x64"));
assert.equal(nonCanonicalApplied.channels["hardgate-darwin-x64"].state, "exact_consumer_verified");
assert.equal(nonCanonicalApplied.channels[CHANNELS.npmWrapper].state, "pending");
for (const packageName of packageNames.filter((name) => name !== linuxPackage)) {
  const applied = applyNativeProof(receiptAt([packageName], "immutable_verified"), proofFor(packageName));
  assert.equal(applied.channels[packageName].state, "exact_consumer_verified");
}

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-native-receipt-"));
try {
  const receiptPath = path.join(directory, "receipt.json");
  const proofPath = path.join(directory, "proof.json");
  writeReceiptAtomicSync(receiptPath, receiptAt(exactChannels, "immutable_verified"), identity);
  writeProof(proofPath, exactProof);
  const proofBytes = fs.readFileSync(proofPath);
  let result = runCli(["--receipt", receiptPath, "--proof", proofPath]);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /native proof applied/);
  assert.deepEqual(fs.readFileSync(proofPath), proofBytes, "CLI must not mutate proof input");
  const appliedBytes = fs.readFileSync(receiptPath);
  assert.equal(readReceipt(receiptPath, identity).channels[linuxPackage].state, "exact_consumer_verified");
  result = runCli(["--receipt", receiptPath, "--proof", proofPath]);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(fs.readFileSync(receiptPath), appliedBytes, "matching replay must preserve receipt bytes");

  const changed = copy(exactProof);
  changed.consumer.sha256 = h64("a");
  changed.wrapper.sha256 = h64("a");
  writeProof(proofPath, changed);
  result = runCli(["--receipt", receiptPath, "--proof", proofPath], { NATIVE_RECEIPT_TEST_SECRET: "credential-sentinel" });
  assert.notEqual(result.status, 0);
  assert.doesNotMatch(result.stderr, /credential-sentinel|[0-9a-f]{64}/);
  assert.deepEqual(fs.readFileSync(receiptPath), appliedBytes, "replay rejection must preserve receipt bytes");

  const defaultPath = path.join(directory, "default.json");
  const defaultProofPath = path.join(directory, "default-proof.json");
  writeReceiptAtomicSync(defaultPath, receiptAt(exactChannels, "promoted"), identity);
  writeProof(defaultProofPath, proofFor(linuxPackage, "default"));
  result = runCli(["--receipt", defaultPath, "--proof", defaultProofPath]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readReceipt(defaultPath, identity).channels[CHANNELS.npmWrapper].state, "default_consumer_verified");

  const malformedPath = path.join(directory, "malformed.json");
  fs.writeFileSync(malformedPath, "{not-json\n");
  const unchangedBeforeMalformed = fs.readFileSync(defaultPath);
  result = runCli(["--receipt", defaultPath, "--proof", malformedPath]);
  assert.notEqual(result.status, 0);
  assert.deepEqual(fs.readFileSync(defaultPath), unchangedBeforeMalformed);

  const oversizedPath = path.join(directory, "oversized.json");
  fs.writeFileSync(oversizedPath, Buffer.alloc(4 * 1024 * 1024 + 1, 0x20));
  result = runCli(["--receipt", defaultPath, "--proof", oversizedPath]);
  assert.notEqual(result.status, 0);
  assert.deepEqual(fs.readFileSync(defaultPath), unchangedBeforeMalformed);

  const proofLink = path.join(directory, "proof-link.json");
  writeProof(proofPath, proofFor("hardgate-darwin-x64"));
  fs.symlinkSync(proofPath, proofLink);
  result = runCli(["--receipt", defaultPath, "--proof", proofLink]);
  assert.notEqual(result.status, 0);
  const proofDirectory = path.join(directory, "proof-directory");
  fs.mkdirSync(proofDirectory);
  result = runCli(["--receipt", defaultPath, "--proof", proofDirectory]);
  assert.notEqual(result.status, 0);
  const receiptLink = path.join(directory, "receipt-link.json");
  fs.symlinkSync(defaultPath, receiptLink);
  result = runCli(["--receipt", receiptLink, "--proof", proofPath]);
  assert.notEqual(result.status, 0);

  const aliasProof = path.join(directory, "receipt-alias.json");
  fs.linkSync(defaultPath, aliasProof);
  result = runCli(["--receipt", defaultPath, "--proof", aliasProof]);
  assert.notEqual(result.status, 0);
  assert.notEqual(runCli(["--receipt", defaultPath]).status, 0);
  assert.notEqual(runCli(["--receipt", defaultPath, "--proof", proofPath, "--extra"]).status, 0);
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}

console.log("native_receipt: strict proof validation, coupled transitions, replay, and atomic CLI failure contracts verified");
