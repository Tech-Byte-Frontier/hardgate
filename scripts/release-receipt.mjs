// Strict, local-only release receipt and publication-state primitives.
//
// This module records evidence supplied by independent release verifiers. It
// does not contact a registry, publish an artifact, or treat a receipt as a
// substitute for those verifiers.
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { isDeepStrictEqual } from "node:util";
import {
  assertChannelName,
  assertExactKeys,
  assertPlainObject,
  clone,
  completeFromChannels,
  fail,
  MAX_RECEIPT_BYTES,
  nextState,
  RECEIPT_SCHEMA_VERSION,
  REQUIRED_CHANNELS,
  validateEvidence,
  validateFailureOperation,
  validateIdentity,
  validateReceipt,
} from "./release-receipt-validation.mjs";

export {
  CHANNELS,
  MAX_RECEIPT_BYTES,
  RECEIPT_SCHEMA_VERSION,
  RECEIPT_STATES,
  REQUIRED_CHANNELS,
  validateReceipt,
} from "./release-receipt-validation.mjs";

function channelNames(input) {
  if (input === undefined) return [...REQUIRED_CHANNELS];
  if (Array.isArray(input)) return [...input];
  if (input !== null && typeof input === "object" && !Array.isArray(input)) return Object.getOwnPropertyNames(input);
  fail("channels must be an array of required channel names");
}

function assertRequiredChannelList(input) {
  const names = channelNames(input);
  if (names.length !== REQUIRED_CHANNELS.length || names.some((name, index) => name !== REQUIRED_CHANNELS[index])) {
    fail("channels must list every required release channel in canonical order");
  }
  return names;
}

export function createReceipt(identity, channels = REQUIRED_CHANNELS) {
  const checkedIdentity = validateIdentity(identity);
  const names = assertRequiredChannelList(channels);
  const checkedChannels = {};
  for (const channel of names) checkedChannels[channel] = { state: "pending", events: [] };
  const receipt = {
    schema_version: RECEIPT_SCHEMA_VERSION,
    identity: checkedIdentity,
    channels: checkedChannels,
    complete: false,
  };
  validateReceipt(receipt, checkedIdentity);
  return receipt;
}

function assertOperationObject(value, keys, label) {
  assertPlainObject(value, label);
  assertExactKeys(value, keys, label);
}

export function recordTransition(receipt, operation) {
  validateReceipt(receipt);
  assertOperationObject(operation, ["channel", "from", "to", "evidence"], "transition");
  const channelName = assertChannelName(operation.channel);
  const evidence = validateEvidence(operation.evidence, receipt.identity);
  const channel = receipt.channels[channelName];
  const existing = channel.events.find((event) => event.type === "transition" && event.from === operation.from && event.to === operation.to);
  if (existing) {
    if (!isDeepStrictEqual(existing.evidence, evidence)) fail("replayed transition evidence does not match the recorded evidence");
    return receipt;
  }
  if (operation.from !== channel.state) fail("transition source state does not match the channel state");
  if (operation.to !== nextState(operation.from)) fail("transition must advance exactly one state");
  channel.events.push({ type: "transition", from: operation.from, to: operation.to, evidence: clone(evidence) });
  channel.state = operation.to;
  receipt.complete = completeFromChannels(receipt.channels);
  validateReceipt(receipt);
  return receipt;
}

export function recordFailure(receipt, operation) {
  validateReceipt(receipt);
  const { channel: channelName, ...failure } = validateFailureOperation(operation, receipt);
  receipt.channels[channelName].events.push({ type: "failure", ...failure });
  validateReceipt(receipt);
  return receipt;
}

export function receiptComplete(receipt) {
  try {
    validateReceipt(receipt);
    return receipt.complete;
  } catch {
    return false;
  }
}

function resolvedReceiptPath(filePath) {
  if (typeof filePath !== "string" || filePath.length === 0) fail("receipt path must be a non-empty string");
  return path.resolve(filePath);
}

function serializedReceipt(receipt, expectedIdentity) {
  validateReceipt(receipt, expectedIdentity);
  const bytes = Buffer.from(`${JSON.stringify(receipt, null, 2)}\n`, "utf8");
  if (bytes.length > MAX_RECEIPT_BYTES) fail("receipt exceeds the JSON size limit");
  return bytes;
}

function assertWritableStats(stats) {
  if (stats.isSymbolicLink()) fail("refusing to overwrite a symbolic-link receipt path");
  if (!stats.isFile()) fail("receipt path must be a regular file");
}

function assertWritableTargetSync(target) {
  try {
    assertWritableStats(fs.lstatSync(target));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}

async function assertWritableTarget(target) {
  try {
    assertWritableStats(await fs.promises.lstat(target));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}

function temporaryPath(target) {
  return path.join(path.dirname(target), `.${path.basename(target)}.${process.pid}.${crypto.randomBytes(12).toString("hex")}.tmp`);
}

function syncDirectory(directory) {
  try {
    const handle = fs.openSync(directory, "r");
    try {
      fs.fsyncSync(handle);
    } finally {
      fs.closeSync(handle);
    }
  } catch (error) {
    if (!(["EINVAL", "ENOTSUP", "EISDIR"].includes(error.code))) throw error;
  }
}

async function syncDirectoryAsync(directory) {
  try {
    const handle = await fs.promises.open(directory, "r");
    try {
      await handle.sync();
    } finally {
      await handle.close();
    }
  } catch (error) {
    if (!(["EINVAL", "ENOTSUP", "EISDIR"].includes(error.code))) throw error;
  }
}

export async function writeReceiptAtomic(filePath, receipt, expectedIdentity) {
  const target = resolvedReceiptPath(filePath);
  const bytes = serializedReceipt(receipt, expectedIdentity);
  const directory = path.dirname(target);
  const temporary = temporaryPath(target);
  await assertWritableTarget(target);
  let handle;
  try {
    handle = await fs.promises.open(temporary, "wx", 0o600);
    await handle.writeFile(bytes);
    await handle.sync();
    await handle.close();
    handle = undefined;
    await assertWritableTarget(target);
    await fs.promises.rename(temporary, target);
    await syncDirectoryAsync(directory);
  } catch (error) {
    if (handle) await handle.close().catch(() => {});
    await fs.promises.unlink(temporary).catch(() => {});
    throw error;
  }
  return receipt;
}

export function writeReceiptAtomicSync(filePath, receipt, expectedIdentity) {
  const target = resolvedReceiptPath(filePath);
  const bytes = serializedReceipt(receipt, expectedIdentity);
  const directory = path.dirname(target);
  const temporary = temporaryPath(target);
  assertWritableTargetSync(target);
  let descriptor;
  try {
    descriptor = fs.openSync(temporary, "wx", 0o600);
    fs.writeFileSync(descriptor, bytes);
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor);
    descriptor = undefined;
    assertWritableTargetSync(target);
    fs.renameSync(temporary, target);
    syncDirectory(directory);
  } catch (error) {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    try { fs.unlinkSync(temporary); } catch (cleanupError) { if (cleanupError.code !== "ENOENT") throw error; }
    throw error;
  }
  return receipt;
}

function parseReceiptBytes(bytes, expectedIdentity, target) {
  if (bytes.length > MAX_RECEIPT_BYTES) fail("receipt exceeds the JSON size limit");
  let value;
  try {
    value = JSON.parse(bytes.toString("utf8"));
  } catch {
    fail(`receipt at ${target} is not valid JSON`);
  }
  validateReceipt(value, expectedIdentity);
  return value;
}

function assertReadableTargetSync(target) {
  const stats = fs.lstatSync(target);
  if (stats.isSymbolicLink()) fail("refusing to read a symbolic-link receipt path");
  if (!stats.isFile()) fail("receipt path must be a regular file");
  if (stats.size > MAX_RECEIPT_BYTES) fail("receipt exceeds the JSON size limit");
}

async function assertReadableTarget(target) {
  const stats = await fs.promises.lstat(target);
  if (stats.isSymbolicLink()) fail("refusing to read a symbolic-link receipt path");
  if (!stats.isFile()) fail("receipt path must be a regular file");
  if (stats.size > MAX_RECEIPT_BYTES) fail("receipt exceeds the JSON size limit");
}

export function readReceipt(filePath, expectedIdentity) {
  const target = resolvedReceiptPath(filePath);
  assertReadableTargetSync(target);
  return parseReceiptBytes(fs.readFileSync(target), expectedIdentity, target);
}

export async function readReceiptAsync(filePath, expectedIdentity) {
  const target = resolvedReceiptPath(filePath);
  await assertReadableTarget(target);
  return parseReceiptBytes(await fs.promises.readFile(target), expectedIdentity, target);
}
