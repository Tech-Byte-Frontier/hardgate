// Strict schema and identity validation for release receipts.
"use strict";

import { PLATFORM_NAMES } from "./release-platforms.mjs";

import { isDeepStrictEqual } from "node:util";
import { compareReleaseTags } from "./release-order.mjs";

export const RECEIPT_SCHEMA_VERSION = 1;
export const RECEIPT_STATES = Object.freeze([
  "pending",
  "staged",
  "immutable_verified",
  "exact_consumer_verified",
  "promoted",
  "default_consumer_verified",
]);
export const CHANNELS = Object.freeze({
  npmPlatforms: PLATFORM_NAMES,
  npmWrapper: "@tech-byte-frontier/hardgate",
  crate: "hardgate",
  githubAssets: "github-assets",
});
export const REQUIRED_CHANNELS = Object.freeze([
  ...CHANNELS.npmPlatforms,
  CHANNELS.npmWrapper,
  CHANNELS.crate,
  CHANNELS.githubAssets,
]);

const RECEIPT_KEYS = ["schema_version", "identity", "channels", "complete"];
const IDENTITY_KEYS = ["version", "source_sha", "tooling_sha", "signed_tag_object", "build_run_id", "artifact_id", "archives"];
const ARCHIVE_KEYS = ["name", "sha256"];
const CHANNEL_KEYS = ["state", "events"];
const TRANSITION_KEYS = ["type", "from", "to", "evidence"];
const EVIDENCE_KEYS = ["version", "source_sha", "archives"];
const CONSUMER_KEYS = ["executable", "sha256"];
const HASH40 = /^[0-9a-f]{40}$/;
const HASH64 = /^[0-9a-f]{64}$/;
const POSITIVE_DECIMAL = /^[1-9][0-9]*$/;
const SAFE_CODE = /^[a-z0-9][a-z0-9._-]{0,63}$/;
const SAFE_ARCHIVE_NAME = /^[A-Za-z0-9][A-Za-z0-9._+@-]*$/;
const SECRET_KEY = /(?:token|secret|password|credential|authorization|private[_-]?key|api[_-]?key)/i;
const SECRET_VALUE = /(?:bearer\s+|(?:token|secret|password|credential|authorization)\s*[:=]|-----begin [^-]*private key-----)/i;
const REQUIRED_CHANNEL_SET = new Set(REQUIRED_CHANNELS);
const STATE_SET = new Set(RECEIPT_STATES);

export const MAX_RECEIPT_BYTES = 4 * 1024 * 1024;

export function fail(message) {
  throw new Error(`release receipt: ${message}`);
}
export function assertPlainObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) fail(`${label} must be an object`);
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) fail(`${label} must be a plain object`);
  if (Object.getOwnPropertySymbols(value).length > 0) fail(`${label} cannot contain symbol keys`);
}

export function assertExactKeys(value, expected, label, { optional = [] } = {}) {
  const allowed = new Set([...expected, ...optional]);
  const actual = Object.getOwnPropertyNames(value);
  if (actual.length !== new Set(actual).size || actual.some((key) => !allowed.has(key))) fail(`${label} contains unknown keys`);
  for (const key of expected) if (!Object.prototype.hasOwnProperty.call(value, key)) fail(`${label}.${key} is required`);
}

function assertNoSecretFields(value, label = "receipt") {
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) assertNoSecretFields(item, `${label}[${index}]`);
    return;
  }
  if (value === null || typeof value !== "object") return;
  assertPlainObject(value, label);
  for (const key of Object.getOwnPropertyNames(value)) {
    if (SECRET_KEY.test(key)) fail(`${label} contains a secret-like field`);
    assertNoSecretFields(value[key], `${label}.${key}`);
  }
}

function assertText(value, label, { max = 4096, allowEmpty = false } = {}) {
  if (typeof value !== "string") fail(`${label} must be a string`);
  if ((!allowEmpty && value.length === 0) || value.length > max) fail(`${label} has an invalid length`);
  if (value.trim() !== value || /[\u0000-\u001f\u007f]/u.test(value)) fail(`${label} contains unsupported characters`);
  if (SECRET_VALUE.test(value)) fail(`${label} looks like a secret`);
  return value;
}

function assertHash(value, label, pattern, width) {
  if (typeof value !== "string" || !pattern.test(value)) fail(`${label} must be ${width} lowercase hexadecimal characters`);
  return value;
}

function assertVersion(value, label = "version") {
  if (typeof value !== "string" || value.length === 0) fail(`${label} must be a semantic version`);
  try {
    compareReleaseTags(`v${value}`, `v${value}`);
  } catch {
    fail(`${label} must be a valid repository semantic version`);
  }
  return value;
}

function assertPositiveDecimal(value, label) {
  if (typeof value !== "string" || !POSITIVE_DECIMAL.test(value)) fail(`${label} must be a positive decimal string`);
  return value;
}

export function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

function validateArchives(value, label = "archives") {
  if (!Array.isArray(value) || value.length === 0 || value.length > 1024) fail(`${label} must be a non-empty array`);
  const archives = [];
  let previousName = null;
  const names = new Set();
  for (const [index, archive] of value.entries()) {
    const archiveLabel = `${label}[${index}]`;
    assertPlainObject(archive, archiveLabel);
    assertExactKeys(archive, ARCHIVE_KEYS, archiveLabel);
    const name = assertText(archive.name, `${archiveLabel}.name`, { max: 512 });
    if (!SAFE_ARCHIVE_NAME.test(name)) fail(`${archiveLabel}.name has an invalid archive name`);
    if (names.has(name)) fail(`${label} contains duplicate archive names`);
    if (previousName !== null && previousName >= name) fail(`${label} must be uniquely ordered by name`);
    names.add(name);
    previousName = name;
    archives.push({ name, sha256: assertHash(archive.sha256, `${archiveLabel}.sha256`, HASH64, 64) });
  }
  return archives;
}

export function validateIdentity(value, label = "identity") {
  assertPlainObject(value, label);
  assertExactKeys(value, IDENTITY_KEYS, label);
  return {
    version: assertVersion(value.version, `${label}.version`),
    source_sha: assertHash(value.source_sha, `${label}.source_sha`, HASH40, 40),
    tooling_sha: assertHash(value.tooling_sha, `${label}.tooling_sha`, HASH40, 40),
    signed_tag_object: assertHash(value.signed_tag_object, `${label}.signed_tag_object`, HASH40, 40),
    build_run_id: assertPositiveDecimal(value.build_run_id, `${label}.build_run_id`),
    artifact_id: assertPositiveDecimal(value.artifact_id, `${label}.artifact_id`),
    archives: validateArchives(value.archives, `${label}.archives`),
  };
}

function validateConsumer(value, label = "consumer") {
  assertPlainObject(value, label);
  assertExactKeys(value, CONSUMER_KEYS, label);
  return {
    executable: assertText(value.executable, `${label}.executable`, { max: 1024 }),
    sha256: assertHash(value.sha256, `${label}.sha256`, HASH64, 64),
  };
}

function requiresConsumerEvidence(destinationState) {
  return destinationState === "exact_consumer_verified" || destinationState === "default_consumer_verified";
}

export function validateEvidence(value, identity, label = "evidence", destinationState) {
  assertPlainObject(value, label);
  assertExactKeys(value, EVIDENCE_KEYS, label, { optional: ["consumer"] });
  const evidence = {
    version: assertVersion(value.version, `${label}.version`),
    source_sha: assertHash(value.source_sha, `${label}.source_sha`, HASH40, 40),
    archives: validateArchives(value.archives, `${label}.archives`),
  };
  if (Object.prototype.hasOwnProperty.call(value, "consumer")) evidence.consumer = validateConsumer(value.consumer, `${label}.consumer`);
  if (requiresConsumerEvidence(destinationState) && !Object.prototype.hasOwnProperty.call(evidence, "consumer")) {
    fail(`${label}.consumer is required for the consumer verification state`);
  }
  if (evidence.version !== identity.version || evidence.source_sha !== identity.source_sha) fail(`${label} does not identify the receipt version and source`);
  if (!isDeepStrictEqual(evidence.archives, identity.archives)) fail(`${label}.archives do not match the receipt artifact digests`);
  return evidence;
}

export function nextState(state) {
  const index = RECEIPT_STATES.indexOf(state);
  return index >= 0 && index < RECEIPT_STATES.length - 1 ? RECEIPT_STATES[index + 1] : null;
}

export function assertChannelName(channel) {
  if (typeof channel !== "string" || !REQUIRED_CHANNEL_SET.has(channel)) fail("unknown release channel");
  return channel;
}

function validateTransitionEvent(event, identity, state, label) {
  assertExactKeys(event, TRANSITION_KEYS, label);
  if (event.from !== state) fail(`${label}.from does not follow the channel state`);
  if (event.to !== nextState(state)) fail(`${label}.to skips or repeats a channel state`);
  if (!STATE_SET.has(event.from) || !STATE_SET.has(event.to)) fail(`${label} contains an unknown state`);
  return {
    state: event.to,
    event: { type: "transition", from: event.from, to: event.to, evidence: validateEvidence(event.evidence, identity, `${label}.evidence`, event.to) },
  };
}

function validateFailureEvent(event, identity, state, label) {
  assertExactKeys(event, ["type", "state", "code", "message"], label, { optional: ["evidence"] });
  if (event.state !== state) fail(`${label}.state does not match the channel state`);
  if (!STATE_SET.has(event.state)) fail(`${label}.state is unknown`);
  if (typeof event.code !== "string" || !SAFE_CODE.test(event.code)) fail(`${label}.code is invalid`);
  const failure = { type: "failure", state, code: event.code, message: assertText(event.message, `${label}.message`, { max: 2000 }) };
  if (Object.prototype.hasOwnProperty.call(event, "evidence")) failure.evidence = validateEvidence(event.evidence, identity, `${label}.evidence`, state);
  return { state, event: failure };
}

function validateEvent(event, identity, state, label) {
  assertPlainObject(event, label);
  if (event.type === "transition") return validateTransitionEvent(event, identity, state, label);
  if (event.type === "failure") return validateFailureEvent(event, identity, state, label);
  fail(`${label}.type is unknown`);
}

function validateChannels(value, identity) {
  assertPlainObject(value, "channels");
  const actual = Object.getOwnPropertyNames(value);
  if (actual.length !== REQUIRED_CHANNELS.length || actual.some((channel) => !REQUIRED_CHANNEL_SET.has(channel))) fail("channels must contain every required release channel exactly once");
  const channels = {};
  for (const channelName of REQUIRED_CHANNELS) {
    const channel = value[channelName];
    const label = `channels.${channelName}`;
    assertPlainObject(channel, label);
    assertExactKeys(channel, ["state", "events"], label);
    if (!STATE_SET.has(channel.state)) fail(`${label}.state is unknown`);
    if (!Array.isArray(channel.events)) fail(`${label}.events must be an array`);
    if (channel.events.length > 4096) fail(`${label}.events must contain at most 4096 entries`);
    let state = "pending";
    const events = [];
    for (const [index, event] of channel.events.entries()) {
      const checked = validateEvent(event, identity, state, `${label}.events[${index}]`);
      state = checked.state;
      events.push(checked.event);
    }
    if (channel.state !== state) fail(`${label}.state does not match its event history`);
    channels[channelName] = { state, events };
  }
  return channels;
}

export function completeFromChannels(channels) {
  return REQUIRED_CHANNELS.every((channel) => channels[channel].state === "default_consumer_verified");
}

export function validateReceipt(value, expectedIdentity) {
  assertPlainObject(value, "receipt");
  assertNoSecretFields(value);
  assertExactKeys(value, RECEIPT_KEYS, "receipt");
  if (value.schema_version !== RECEIPT_SCHEMA_VERSION) fail("unsupported schema_version");
  const identity = validateIdentity(value.identity);
  if (expectedIdentity !== undefined && !isDeepStrictEqual(identity, validateIdentity(expectedIdentity, "expectedIdentity"))) fail("receipt identity does not match the expected release identity");
  const channels = validateChannels(value.channels, identity);
  if (typeof value.complete !== "boolean") fail("receipt.complete must be boolean");
  if (value.complete !== completeFromChannels(channels)) fail("receipt.complete does not match all channel states");
  return value;
}

export function validateFailureOperation(operation, receipt) {
  assertPlainObject(operation, "failure");
  const actual = Object.getOwnPropertyNames(operation);
  if (actual.some((key) => !["channel", "code", "message", "evidence"].includes(key))) fail("failure contains unknown keys");
  for (const key of ["channel", "code", "message"]) if (!Object.prototype.hasOwnProperty.call(operation, key)) fail(`failure.${key} is required`);
  const channel = assertChannelName(operation.channel);
  if (typeof operation.code !== "string" || !SAFE_CODE.test(operation.code)) fail("failure.code is invalid");
  const result = { channel, state: receipt.channels[channel].state, code: operation.code, message: assertText(operation.message, "failure.message", { max: 2000 }) };
  if (Object.prototype.hasOwnProperty.call(operation, "evidence")) result.evidence = validateEvidence(operation.evidence, receipt.identity, "failure.evidence", result.state);
  return result;
}
