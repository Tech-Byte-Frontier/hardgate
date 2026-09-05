// Public exact-version probes shared by publication and propagation recovery.
"use strict";
import { setTimeout as delay } from "node:timers/promises";
import { childTimeoutMs, remainingMs } from "./npm-verification-policy.mjs";
import { isRetryableNpmPackError, retryAfterMs } from "./npm-pack-retry.mjs";
import { runReleaseProcess } from "./release-process.mjs";

function parseResponse(text, name, version) {
  text = text.trimEnd();
  const boundary = text.lastIndexOf("\n");
  const status = Number(text.slice(boundary + 1));
  const response = text.slice(0, boundary);
  const separator = response.lastIndexOf("\r\n\r\n");
  const body = separator < 0 ? response : response.slice(separator + 4);
  const retryAfter = response.slice(0, separator).match(/^retry-after:\s*(.+)$/im)?.[1] ?? "0";
  if (status === 404) return { state: "missing" };
  if (status !== 200) throw Object.assign(new Error(`npm exact-version endpoint returned HTTP ${status}\nRetry-After: ${retryAfter}`), { code: `E${status}` });
  const metadata = JSON.parse(body);
  if (metadata.name !== name || metadata.version !== version) throw new Error("npm exact-version metadata identity mismatch");
  return { state: "present" };
}

export async function probeNpmVersion(request) {
  const { name, version, policy } = request;
  const url = `https://registry.npmjs.org/${encodeURIComponent(name)}/${version}`;
  const text = await runReleaseProcess("curl", ["--silent", "--show-error", "--include", "--connect-timeout", "10", "--write-out", "\n%{http_code}", url], { timeoutMs: childTimeoutMs(policy), env: request.env ?? process.env });
  return parseResponse(text, name, version);
}

export async function registryBackoff(policy, error) {
  const pause = Math.max(policy.delayMs, retryAfterMs(error));
  if (pause >= remainingMs(policy)) throw new Error("npm registry retry exceeds operation deadline");
  await delay(pause);
}

export async function waitForNpmVersion(request, retryMissing = false, probe = probeNpmVersion) {
  for (let attempt = 1; attempt <= request.policy.attempts; attempt += 1) {
    remainingMs(request.policy);
    let failure;
    try {
      const result = await probe(request);
      if (result.state === "present" || !retryMissing) return result;
      failure = new Error("npm exact version is still missing");
    } catch (error) {
      if (!isRetryableNpmPackError(error)) throw error;
      failure = error;
    }
    if (attempt === request.policy.attempts) throw failure;
    await registryBackoff(request.policy, failure);
  }
  throw new Error("npm version probe requires at least one attempt");
}
