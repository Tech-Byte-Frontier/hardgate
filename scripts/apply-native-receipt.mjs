#!/usr/bin/env node
// Apply a native worker's caller-supplied proof to a local release receipt.
// The adapter records proof; it does not independently execute or attest it.
"use strict";

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  CHANNELS,
  MAX_RECEIPT_BYTES,
  RECEIPT_SCHEMA_VERSION,
  assertExactKeys,
  assertPlainObject,
  clone,
  fail,
  validateReceipt,
} from "./release-receipt-validation.mjs";
import { readReceipt, recordTransition, writeReceiptAtomicSync } from "./release-receipt.mjs";

const HASH40 = /^[0-9a-f]{40}$/;
const HASH64 = /^[0-9a-f]{64}$/;
const PROOF_MAX_BYTES = MAX_RECEIPT_BYTES;
const READ_FLAGS = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0);
const READ_CHUNK_BYTES = 64 * 1024;
const PACKAGE_NAMES = new Set(CHANNELS.npmPlatforms);
const WRAPPER_PACKAGE = "hardgate-linux-x64";
const WRAPPER_CHANNEL = CHANNELS.npmWrapper;

function assertHash(value, pattern, label) {
  if (typeof value !== "string" || !pattern.test(value)) fail(`${label} is invalid`);
  return value;
}

function assertProofSize(proof) {
  let bytes;
  try {
    bytes = Buffer.byteLength(JSON.stringify(proof), "utf8");
  } catch {
    fail("proof is not serializable");
  }
  if (bytes > PROOF_MAX_BYTES) fail("proof exceeds the JSON size limit");
}

function expectedConsumerPath(packageName) {
  return `node_modules/${packageName}/bin/hardgate`;
}

function transitionFor(mode) {
  if (mode === "exact") return { from: "immutable_verified", to: "exact_consumer_verified" };
  if (mode === "default") return { from: "promoted", to: "default_consumer_verified" };
  fail("proof.mode is invalid");
}

function validateProofIdentity(proof, identity) {
  if (proof.schema_version !== RECEIPT_SCHEMA_VERSION) fail("proof.schema_version is unsupported");
  if (proof.version !== identity.version) fail("proof.version does not match the receipt");
  assertHash(proof.source_sha, HASH40, "proof.source_sha");
  if (proof.source_sha !== identity.source_sha) fail("proof.source_sha does not match the receipt");
  const transition = transitionFor(proof.mode);
  if (!PACKAGE_NAMES.has(proof.package)) fail("proof.package is unsupported");
  return transition;
}

function validateArchiveProof(proof, identity) {
  const archiveName = `${proof.package}.tar.gz`;
  assertPlainObject(proof.archive, "proof.archive");
  assertExactKeys(proof.archive, ["name", "sha256"], "proof.archive");
  if (proof.archive.name !== archiveName) fail("proof.archive.name does not match the package");
  assertHash(proof.archive.sha256, HASH64, "proof.archive.sha256");
  const expectedArchive = identity.archives.find((archive) => archive.name === archiveName);
  if (!expectedArchive || proof.archive.sha256 !== expectedArchive.sha256) {
    fail("proof.archive does not match the receipt artifact");
  }
  return { name: proof.archive.name, sha256: proof.archive.sha256 };
}

function validateConsumerProof(proof) {
  const consumerPath = expectedConsumerPath(proof.package);
  assertPlainObject(proof.consumer, "proof.consumer");
  assertExactKeys(proof.consumer, ["executable", "sha256"], "proof.consumer");
  if (proof.consumer.executable !== consumerPath) fail("proof.consumer.executable is invalid");
  const consumerHash = assertHash(proof.consumer.sha256, HASH64, "proof.consumer.sha256");
  return { executable: proof.consumer.executable, sha256: consumerHash };
}

function validateWrapperProof(proof, consumerHash) {
  const hasWrapper = Object.hasOwn(proof, "wrapper");
  if (proof.package === WRAPPER_PACKAGE && !hasWrapper) fail("proof.wrapper is required for the canonical package");
  if (proof.package !== WRAPPER_PACKAGE && hasWrapper) fail("proof.wrapper is forbidden for this package");
  if (!hasWrapper) return undefined;
  assertPlainObject(proof.wrapper, "proof.wrapper");
  assertExactKeys(proof.wrapper, ["executable", "sha256"], "proof.wrapper");
  if (proof.wrapper.executable !== expectedConsumerPath(WRAPPER_PACKAGE)) fail("proof.wrapper.executable is invalid");
  if (proof.wrapper.sha256 !== consumerHash) fail("proof.wrapper.sha256 must match the consumer proof");
  return { executable: proof.wrapper.executable, sha256: proof.wrapper.sha256 };
}

function validateProof(proof, identity) {
  assertPlainObject(proof, "proof");
  assertProofSize(proof);
  assertExactKeys(proof, ["schema_version", "version", "source_sha", "mode", "package", "archive", "consumer"], "proof", {
    optional: ["wrapper"],
  });
  const transition = validateProofIdentity(proof, identity);
  const archive = validateArchiveProof(proof, identity);
  const consumer = validateConsumerProof(proof);
  const wrapper = validateWrapperProof(proof, consumer.sha256);
  return {
    schema_version: RECEIPT_SCHEMA_VERSION,
    version: proof.version,
    source_sha: proof.source_sha,
    mode: proof.mode,
    package: proof.package,
    archive,
    consumer,
    ...(wrapper ? { wrapper } : {}),
    transition,
  };
}

function transitionEvidence(receipt, consumer) {
  return {
    version: receipt.identity.version,
    source_sha: receipt.identity.source_sha,
    archives: clone(receipt.identity.archives),
    consumer: clone(consumer),
  };
}

function assertPhase(receipt, channels, transition) {
  const states = channels.map((channel) => receipt.channels[channel].state);
  const atSource = states.every((state) => state === transition.from);
  const atTarget = states.every((state) => state === transition.to);
  if (!atSource && !atTarget) fail("receipt channels are not at the adjacent native checkpoint");
}

function applyChannelTransition(receipt, channel, transition, evidence) {
  recordTransition(receipt, { channel, from: transition.from, to: transition.to, evidence });
}

export function applyNativeProof(receipt, proof) {
  validateReceipt(receipt);
  const checked = validateProof(proof, receipt.identity);
  const proposed = clone(receipt);
  const channels = [checked.package];
  if (checked.wrapper) channels.push(WRAPPER_CHANNEL);
  assertPhase(proposed, channels, checked.transition);
  const evidence = transitionEvidence(proposed, checked.consumer);

  // Both transitions operate on the copy. The caller writes it atomically only
  // after this function returns, so a coupled wrapper update cannot half-commit.
  applyChannelTransition(proposed, checked.package, checked.transition, evidence);
  if (checked.wrapper) applyChannelTransition(proposed, WRAPPER_CHANNEL, checked.transition, evidence);
  validateReceipt(proposed);
  return proposed;
}

function parseArguments(argv) {
  const options = Object.create(null);
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument !== "--receipt" && argument !== "--proof") fail("arguments are --receipt FILE --proof FILE");
    if (Object.hasOwn(options, argument)) fail("an argument was specified more than once");
    const value = argv[index + 1];
    if (!value || value.startsWith("-")) fail("each argument requires a file");
    options[argument] = value;
    index += 1;
  }
  if (!options["--receipt"] || !options["--proof"]) fail("--receipt and --proof are required");
  return { receipt: options["--receipt"], proof: options["--proof"] };
}

function inputStats(file, label) {
  let stats;
  try {
    stats = fs.lstatSync(path.resolve(file));
  } catch {
    fail(`${label} cannot be read`);
  }
  if (stats.isSymbolicLink() || !stats.isFile()) fail(`${label} must be a regular file`);
  return stats;
}

function sameFile(left, right) {
  const sameInode = Number.isSafeInteger(left.dev) && Number.isSafeInteger(left.ino)
    && left.dev === right.dev && left.ino === right.ino && left.ino > 0;
  return sameInode;
}

function assertDistinctInputs(receiptPath, proofPath) {
  const receiptTarget = path.resolve(receiptPath);
  const proofTarget = path.resolve(proofPath);
  const receiptStats = inputStats(receiptTarget, "receipt");
  const proofStats = inputStats(proofTarget, "proof");
  if (sameFile(receiptStats, proofStats)) fail("receipt and proof must be distinct files");
  try {
    if (fs.realpathSync(receiptTarget) === fs.realpathSync(proofTarget)) fail("receipt and proof must be distinct files");
  } catch {
    fail("receipt and proof cannot be resolved");
  }
}

function readProof(proofPath) {
  const target = path.resolve(proofPath);
  inputStats(target, "proof");
  let descriptor;
  try {
    descriptor = fs.openSync(target, READ_FLAGS);
    return parseProof(readBoundedProof(descriptor));
  } catch (error) {
    preserveProofError(error);
  } finally {
    closeDescriptor(descriptor);
  }
}

function readBoundedProof(descriptor) {
  const opened = fs.fstatSync(descriptor);
  if (!opened.isFile()) fail("proof must be a regular file");
  if (!Number.isSafeInteger(opened.size) || opened.size > PROOF_MAX_BYTES) fail("proof exceeds the JSON size limit");
  const chunks = [];
  let total = 0;
  while (true) {
    const chunk = Buffer.allocUnsafe(READ_CHUNK_BYTES);
    const bytesRead = fs.readSync(descriptor, chunk, 0, chunk.length, null);
    if (bytesRead === 0) break;
    total += bytesRead;
    if (total > PROOF_MAX_BYTES) fail("proof exceeds the JSON size limit");
    chunks.push(chunk.subarray(0, bytesRead));
  }
  return Buffer.concat(chunks, total);
}

function parseProof(bytes) {
  try {
    return JSON.parse(bytes.toString("utf8"));
  } catch {
    fail("proof is not valid JSON");
  }
}

function preserveProofError(error) {
  if (error instanceof Error && error.message.startsWith("release receipt:")) throw error;
  fail("proof cannot be read");
}

function closeDescriptor(descriptor) {
  if (descriptor !== undefined) fs.closeSync(descriptor);
}

function reportFailure() {
  process.stderr.write("release receipt: native proof rejected\n");
  process.exitCode = 1;
}

function main() {
  try {
    const options = parseArguments(process.argv.slice(2));
    assertDistinctInputs(options.receipt, options.proof);
    const receipt = readReceipt(options.receipt);
    const proof = readProof(options.proof);
    const updated = applyNativeProof(receipt, proof);
    writeReceiptAtomicSync(options.receipt, updated, receipt.identity);
    console.log("release receipt: native proof applied");
  } catch {
    reportFailure();
  }
}

const invokedDirectly = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) main();
