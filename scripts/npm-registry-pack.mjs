// Retrieve exact npm bytes without publishing or trusting install metadata alone.
"use strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { isRetryableNpmPackError, npmErrorText, retryAfterMs } from "./npm-pack-retry.mjs";
import { childTimeoutMs, remainingMs } from "./npm-verification-policy.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import { projectRoot } from "./release-support.mjs";

async function exactVersionObserved(name, version, policy) {
  const url = `https://registry.npmjs.org/${encodeURIComponent(name)}/${version}`;
  const text = await runReleaseProcess("curl", ["--silent", "--show-error", "--fail-with-body", "--connect-timeout", "10", url], { timeoutMs: childTimeoutMs(policy) });
  const metadata = JSON.parse(text);
  if (metadata.name !== name || metadata.version !== version) throw new Error("npm exact-version metadata identity mismatch");
  return true;
}

async function packOnce(spec, directory, policy) {
  await runReleaseProcess("npm", ["pack", spec, "--ignore-scripts", "--loglevel=error", "--pack-destination", directory], {
    cwd: projectRoot,
    timeoutMs: childTimeoutMs(policy),
    env: { ...process.env, npm_config_registry: "https://registry.npmjs.org", npm_config_audit: "false", npm_config_fund: "false", npm_config_fetch_retries: "0" },
  });
  const archives = fs.readdirSync(directory).filter((name) => name.endsWith(".tgz"));
  if (archives.length !== 1) throw new Error(`npm pack ${spec} produced ${archives.length} tarballs`);
  return path.join(directory, archives[0]);
}

async function mayRetry(error, request) {
  if (isRetryableNpmPackError(error)) return true;
  if (!isRetryableNpmPackError(error, { exactVersionObserved: true })) return false;
  // ETARGET is retryable only after this independent endpoint identifies the
  // same requested name and version. Missing, malformed, auth and identity
  // failures stay red; none can trigger publication.
  const observed = await exactVersionObserved(request.name, request.version, request.policy);
  return isRetryableNpmPackError(error, { exactVersionObserved: observed });
}

export async function packRegistryPackage(name, version, policy) {
  const spec = `${name}@=${version}`;
  let lastError;
  for (let attempt = 1; attempt <= policy.attempts; attempt += 1) {
    remainingMs(policy);
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-npm-pack-"));
    try {
      return { archive: await packOnce(spec, directory, policy), directory };
    } catch (error) {
      fs.rmSync(directory, { recursive: true, force: true });
      lastError = npmErrorText(error);
      if (!await mayRetry(error, { name, version, policy })) throw new Error(`npm pack ${spec} failed without retry: ${lastError}`);
      if (attempt === policy.attempts) break;
      const pause = Math.max(policy.delayMs, retryAfterMs(error));
      if (pause >= remainingMs(policy)) throw new Error(`npm pack ${spec} retry exceeds operation deadline`);
      await delay(pause);
    }
  }
  throw new Error(`npm pack ${spec} failed after ${policy.attempts} bounded attempts: ${lastError}`);
}
