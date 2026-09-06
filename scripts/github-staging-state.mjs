// Strict state transitions for staging an immutable public GitHub release.
"use strict";

import { PLATFORM_ASSETS as RELEASE_ASSETS } from "./release-platforms.mjs";

import { performance } from "node:perf_hooks";
import { assertExactKeys as assertKeys, assertPlainObject as assertObject } from "./release-receipt-validation.mjs";
import {
  assertByteProof,
  assertOperations as assertOperationCallbacks,
  assertSemanticVersion,
  validateProbeEnvelope,
} from "./release-state-validation.mjs";

const GITHUB_ARCHIVES = RELEASE_ASSETS;

const SAFE_ASSET = /^[A-Za-z0-9][A-Za-z0-9._+@-]*$/;
const OPERATION_METHODS = ["probe", "create", "upload", "verify"];

function fail(message) {
  throw new Error(`GitHub staging: ${message}`);
}

function assertVersion(value, label) {
  return assertSemanticVersion(value, label, fail);
}

function assertAssetName(value, label) {
  if (typeof value !== "string" || !SAFE_ASSET.test(value) || value === "." || value === "..") fail(`${label} is not a safe asset basename`);
  return value;
}

function assertAssetList(value, label, { allowEmpty = false } = {}) {
  if (!Array.isArray(value) || (!allowEmpty && value.length === 0) || value.length > 64) fail(`${label} must be ${allowEmpty ? "an asset list" : "a non-empty asset list"}`);
  const result = [];
  const names = new Set();
  for (const [index, name] of value.entries()) {
    const checked = assertAssetName(name, `${label}[${index}]`);
    if (names.has(checked)) fail(`${label} contains duplicate assets`);
    names.add(checked);
    result.push(checked);
  }
  return result;
}

export function expectedGithubAssets(version) {
  const checked = assertVersion(version, "version");
  return [...GITHUB_ARCHIVES, "SHA256SUMS", `hardgate-${checked}.sbom.cdx.json`];
}

function assertExpectedAssets(version, assets) {
  const expected = expectedGithubAssets(version);
  if (assets.length !== expected.length || expected.some((name) => !assets.includes(name))) fail("request.assets must exactly match the eight expected release assets");
}

function assertPolicy(policy) {
  assertObject(policy, "request.policy");
  if (!Number.isFinite(policy.deadline)) fail("request.policy.deadline must be a finite monotonic deadline");
  return policy;
}

function assertRequest(request) {
  assertObject(request, "request");
  const expected = ["tag", "version", "assets", "policy"];
  const optional = ["repo"];
  const keys = Object.getOwnPropertyNames(request);
  if (keys.some((key) => !expected.includes(key) && !optional.includes(key))) fail("request contains unknown keys");
  for (const key of expected) if (!Object.prototype.hasOwnProperty.call(request, key)) fail(`request.${key} is required`);
  const version = assertVersion(request.version, "request.version");
  if (typeof request.tag !== "string" || request.tag !== `v${version}`) fail("request.tag must exactly match request.version");
  if (request.repo !== undefined && (typeof request.repo !== "string" || !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(request.repo))) fail("request.repo must be OWNER/REPO");
  const assets = assertAssetList(request.assets, "request.assets");
  assertExpectedAssets(version, assets);
  assertPolicy(request.policy);
  return request;
}

function assertOperations(operations) {
  return assertOperationCallbacks(operations, OPERATION_METHODS, { assertObject, fail });
}

function validatePresentProbe(value) {
  if (typeof value.tag !== "string" || typeof value.isDraft !== "boolean" || typeof value.isPrerelease !== "boolean") fail("probe result metadata is malformed");
  return { state: "present", tag: value.tag, isDraft: value.isDraft, isPrerelease: value.isPrerelease, assets: assertAssetList(value.assets, "probe result.assets", { allowEmpty: true }) };
}

function validateProbe(value) {
  return validateProbeEnvelope(value, {
    assertObject,
    assertKeys,
    fail,
    presentKeys: ["state", "tag", "isDraft", "isPrerelease", "assets"],
    validatePresent: validatePresentProbe,
  });
}

function remainingMs(policy) {
  const remaining = Math.floor(policy.deadline - performance.now());
  if (remaining < 1) fail("operation deadline exhausted");
  return remaining;
}

export { remainingMs };

async function call(request, operation) {
  remainingMs(request.policy);
  const result = await operation();
  remainingMs(request.policy);
  return result;
}

function assertCurrent(request, value) {
  const current = validateProbe(value);
  if (current.state === "missing") fail(`release ${request.tag} is missing`);
  if (current.tag !== request.tag) fail(`release tag mismatch: expected ${request.tag}, got ${current.tag}`);
  if (current.isDraft) fail(`release ${request.tag} is a draft`);
  const expected = new Set(request.assets);
  if (current.assets.some((name) => !expected.has(name))) fail(`release ${request.tag} contains unexpected assets`);
  return current;
}

function assertExactAssets(request, current) {
  if (current.assets.length !== request.assets.length || request.assets.some((name) => !current.assets.includes(name))) fail(`release ${request.tag} does not contain the exact expected asset set`);
}

function assertProof(result, label) {
  assertByteProof(result, fail, `${label} did not verify expected bytes`);
}

async function verifyAssets(request, operations, names) {
  if (names.length === 0) return;
  assertProof(await call(request, () => operations.verify(request, [...names])), "asset verification");
}

async function reconcileRelease(request, operations, requirePrerelease = false) {
  const current = assertCurrent(request, await call(request, () => operations.probe(request)));
  assertExactAssets(request, current);
  if (requirePrerelease && !current.isPrerelease) fail(`new release ${request.tag} is not a public prerelease`);
  await verifyAssets(request, operations, request.assets);
  return current;
}

async function stageMissing(request, operations) {
  let createError;
  try {
    await call(request, () => operations.create(request));
  } catch (error) {
    createError = error;
  }
  const current = await reconcileRelease(request, operations, !createError);
  return { publication: createError ? "ambiguous" : "created", state: "immutable_verified", prerelease: current.isPrerelease };
}

async function stageExisting(request, operations, initial) {
  const existing = new Set(initial.assets);
  await verifyAssets(request, operations, initial.assets);
  const missing = request.assets.filter((name) => !existing.has(name));
  let ambiguous = false;
  for (const name of missing) {
    let uploadError;
    try {
      await call(request, () => operations.upload(request, name));
    } catch (error) {
      uploadError = error;
    }
    const observed = assertCurrent(request, await call(request, () => operations.probe(request)));
    if (!observed.assets.includes(name)) {
      if (uploadError) throw uploadError;
      fail(`uploaded asset ${name} was not observed`);
    }
    await verifyAssets(request, operations, [name]);
    if (uploadError) ambiguous = true;
  }
  if (missing.length === 0) {
    const current = await reconcileRelease(request, operations);
    return { publication: "existing", state: "immutable_verified", prerelease: current.isPrerelease };
  }
  const current = await reconcileRelease(request, operations);
  return { publication: ambiguous ? "ambiguous" : "existing", state: "immutable_verified", prerelease: current.isPrerelease };
}

export async function stageGithubRelease(request, operations) {
  assertRequest(request);
  assertOperations(operations);
  const initial = validateProbe(await call(request, () => operations.probe(request)));
  if (initial.state === "missing") return stageMissing(request, operations);
  return stageExisting(request, operations, assertCurrent(request, initial));
}
