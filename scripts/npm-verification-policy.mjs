// One monotonic operation deadline also bounds each subprocess and backoff.
"use strict";
import { performance } from "node:perf_hooks";
import { compareReleaseTags } from "./release-order.mjs";

function integerSetting(environment, name, fallback, [minimum, maximum]) {
  const text = environment[name] ?? String(fallback);
  const value = Number(text);
  if (!/^\d+$/.test(text) || !Number.isSafeInteger(value) || value < minimum || value > maximum) {
    throw new Error(`${name} must be an integer between ${minimum} and ${maximum}`);
  }
  return value;
}

export function verificationPolicy(version, environment = process.env) {
  compareReleaseTags(`v${version}`, `v${version}`);
  const attempts = integerSetting(environment, "NPM_VERIFY_ATTEMPTS", 20, [1, 100]);
  const delayMs = integerSetting(environment, "NPM_VERIFY_DELAY_SECONDS", 10, [0, 60]) * 1000;
  const timeoutMs = integerSetting(environment, "NPM_VERIFY_TIMEOUT_SECONDS", 600, [1, 3600]) * 1000;
  const childMs = integerSetting(environment, "NPM_VERIFY_CHILD_TIMEOUT_SECONDS", 30, [1, 300]) * 1000;
  return { attempts, delayMs, childMs, deadline: performance.now() + timeoutMs };
}

export function remainingMs(policy) {
  const remaining = Math.floor(policy.deadline - performance.now());
  if (remaining < 1) throw new Error("npm verification operation deadline exhausted");
  return remaining;
}

export function childTimeoutMs(policy) {
  return Math.min(policy.childMs, remainingMs(policy));
}
