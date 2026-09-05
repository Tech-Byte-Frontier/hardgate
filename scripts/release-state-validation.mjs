// Shared strict validation mechanics for publication-state helpers.
"use strict";

import { compareReleaseTags } from "./release-order.mjs";

export function assertSemanticVersion(value, label, fail) {
  if (typeof value !== "string" || value.length === 0) fail(`${label} must be a semantic version`);
  try {
    compareReleaseTags(`v${value}`, `v${value}`);
  } catch {
    fail(`${label} must be a valid repository semantic version`);
  }
  return value;
}

export function assertOperations(operations, requiredMethods, { assertObject, fail }) {
  assertObject(operations, "operations");
  for (const name of requiredMethods) {
    if (typeof operations[name] !== "function") fail(`operations.${name} must be a function`);
  }
  return operations;
}

export function validateProbeEnvelope(value, { assertObject, assertKeys, fail, presentKeys, validatePresent }) {
  assertObject(value, "probe result");
  if (value.state === "missing") {
    assertKeys(value, ["state"], "probe result");
    return { state: "missing" };
  }
  if (value.state !== "present") fail("probe result.state is unknown");
  assertKeys(value, presentKeys, "probe result");
  return validatePresent(value);
}

export function assertByteProof(result, fail, message) {
  if (result === false || (result && typeof result === "object" && result.verified === false)) fail(message);
}
