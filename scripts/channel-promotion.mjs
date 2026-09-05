// Verify an immutable release before advancing one public default channel.
"use strict";

import { performance } from "node:perf_hooks";
import { setTimeout as delay } from "node:timers/promises";
import { compareReleaseTags } from "./release-order.mjs";
import {
  assertByteProof,
  assertOperations as assertOperationCallbacks,
  assertSemanticVersion,
  validateProbeEnvelope,
} from "./release-state-validation.mjs";

const OPERATION_METHODS = ["probe", "promote", "verifyDefault", "verifyImmutable"];

function fail(message) {
  throw new Error(`channel promotion: ${message}`);
}

function assertPlainObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value) || Object.getPrototypeOf(value) !== Object.prototype) {
    fail(`${label} must be an object`);
  }
}

function assertKeys(value, expected, label) {
  const keys = Object.getOwnPropertyNames(value);
  if (keys.length !== expected.length || keys.some((key) => !expected.includes(key))) fail(`${label} contains unknown keys`);
  for (const key of expected) if (!Object.prototype.hasOwnProperty.call(value, key)) fail(`${label}.${key} is required`);
}

function assertVersion(value, label) {
  return assertSemanticVersion(value, label, fail);
}

function assertPolicy(policy) {
  assertPlainObject(policy, "request.policy");
  if (!Number.isSafeInteger(policy.attempts) || policy.attempts < 1 || policy.attempts > 100) fail("request.policy.attempts must be an integer between 1 and 100");
  if (!Number.isFinite(policy.delayMs) || policy.delayMs < 0) fail("request.policy.delayMs must be a non-negative number");
  if (!Number.isFinite(policy.deadline)) fail("request.policy.deadline must be a finite monotonic deadline");
  return policy;
}

function assertRequest(request) {
  assertPlainObject(request, "request");
  assertKeys(request, ["version", "exactConsumerVerified", "policy"], "request");
  assertVersion(request.version, "request.version");
  if (typeof request.exactConsumerVerified !== "boolean") fail("request.exactConsumerVerified must be boolean");
  assertPolicy(request.policy);
  return request;
}

function assertOperations(operations) {
  return assertOperationCallbacks(operations, OPERATION_METHODS, { assertObject: assertPlainObject, fail });
}

function validateProbe(value) {
  return validateProbeEnvelope(value, {
    assertObject: assertPlainObject,
    assertKeys,
    fail,
    presentKeys: ["state", "version"],
    validatePresent: (result) => ({ state: "present", version: assertVersion(result.version, "probe result.version") }),
  });
}

function remainingMs(policy) {
  const remaining = Math.floor(policy.deadline - performance.now());
  if (remaining < 1) fail("policy deadline exhausted");
  return remaining;
}

async function backoff(policy) {
  if (policy.delayMs === 0) return;
  if (policy.delayMs >= remainingMs(policy)) fail("policy delay exceeds remaining deadline");
  await delay(policy.delayMs);
}

function errorText(error) {
  return typeof error === "string" ? error : error?.message ?? String(error);
}

function temporaryProbeFailure(error) {
  if (error?.temporary === true || error?.retryable === true) return true;
  return /\b(?:EAI_AGAIN|ECONNRESET|ETIMEDOUT|ECONNREFUSED|E429|E5\d\d|HTTP(?:\/\d(?:\.\d)?)?\s*(?:404|429|5\d\d)|\b404\b)\b/i.test(error?.code ?? errorText(error));
}

async function readProbe(request, operations) {
  const result = await operations.probe(request);
  remainingMs(request.policy);
  return validateProbe(result);
}

async function probeCurrent(request, operations) {
  let lastError;
  for (let attempt = 1; attempt <= request.policy.attempts; attempt += 1) {
    remainingMs(request.policy);
    try {
      const observed = await readProbe(request, operations);
      if (observed.state === "present") compareObservedVersion(request, observed);
      return observed;
    } catch (error) {
      if (!temporaryProbeFailure(error)) throw error;
      lastError = error;
    }
    if (attempt < request.policy.attempts) await backoff(request.policy);
  }
  throw new Error(`probe failed after ${request.policy.attempts} attempts: ${errorText(lastError)}`);
}

function compareObservedVersion(request, observed) {
  const comparison = compareReleaseTags(`v${observed.version}`, `v${request.version}`);
  if (comparison > 0) fail(`observed version ${observed.version} is newer than requested ${request.version}`);
  if (comparison === 0 && observed.version !== request.version) fail(`observed version ${observed.version} is not the exact requested version ${request.version}`);
  return comparison;
}

async function observeTarget(request, operations) {
  let lastObservation;
  let lastError;
  for (let attempt = 1; attempt <= request.policy.attempts; attempt += 1) {
    remainingMs(request.policy);
    try {
      const observed = await readProbe(request, operations);
      lastObservation = observed;
      const comparison = observed.state === "present" ? compareObservedVersion(request, observed) : -1;
      if (observed.state === "present" && comparison === 0 && observed.version === request.version) return observed;
    } catch (error) {
      if (!temporaryProbeFailure(error)) throw error;
      lastError = error;
    }
    if (attempt < request.policy.attempts) await backoff(request.policy);
  }
  if (lastError) throw new Error(`target probe failed after promotion: ${errorText(lastError)}`);
  throw new Error(`requested version was not observed after promotion: ${lastObservation?.version ?? "missing"}`);
}

function assertProof(result, label) {
  assertByteProof(result, fail, `${label} did not verify the expected bytes`);
}

async function verifyDefault(request, operations) {
  remainingMs(request.policy);
  const result = await operations.verifyDefault(request);
  remainingMs(request.policy);
  assertProof(result, "default consumer verification");
}

function requireExactVersion(request, observed, label) {
  if (observed.state !== "present" || observed.version !== request.version) fail(`${label} no longer exposes the requested version`);
}

export async function promoteVerifiedChannel(request, operations) {
  assertRequest(request);
  assertOperations(operations);
  if (request.exactConsumerVerified !== true) fail("exact consumer verification is required before promotion");
  const initial = await probeCurrent(request, operations);
  remainingMs(request.policy);
  assertProof(await operations.verifyImmutable(request), "immutable verification");
  remainingMs(request.policy);
  const existing = initial.state === "present" && initial.version === request.version;
  if (existing) {
    await verifyDefault(request, operations);
    requireExactVersion(request, await probeCurrent(request, operations), "default channel");
    return { publication: "existing", state: "default_consumer_verified" };
  }
  let promotionError;
  try {
    assertProof(await operations.promote(request), "promotion");
  } catch (error) {
    promotionError = error;
  }
  await observeTarget(request, operations);
  await verifyDefault(request, operations);
  return { publication: promotionError ? "ambiguous" : "promoted", state: "default_consumer_verified" };
}
