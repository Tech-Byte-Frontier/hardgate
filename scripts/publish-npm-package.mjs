#!/usr/bin/env node
// Invoked only by the authorized release workflow after immutable staging.
"use strict";

import { PLATFORM_NAMES } from "./release-platforms.mjs";
import fs from "node:fs";
import path from "node:path";
import { verificationPolicy, childTimeoutMs, remainingMs } from "./npm-verification-policy.mjs";
import { option, projectRoot } from "./release-support.mjs";
import { publishVerifiedPackage } from "./npm-publication-state.mjs";
import { probeNpmVersion } from "./npm-registry-state.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import { npmPublisherAuth, validateNpmPublisherToolchain } from "./npm-publisher-auth.mjs";

const auth = npmPublisherAuth(option("--auth-mode", process.env.HARDGATE_NPM_AUTH_MODE));
if (!auth.publishEnv.ACTIONS_ID_TOKEN_REQUEST_TOKEN || !auth.publishEnv.ACTIONS_ID_TOKEN_REQUEST_URL) {
  throw new Error("npm provenance publication requires GitHub OIDC credentials");
}
const directoryOption = option("--package-dir");
if (!directoryOption) throw new Error("--package-dir is required");
const directory = path.resolve(directoryOption);
const manifest = JSON.parse(fs.readFileSync(path.join(directory, "package.json"), "utf8"));
const version = option("--version");
const policy = verificationPolicy(version, { ...process.env, NPM_VERIFY_ATTEMPTS: process.env.NPM_VERIFY_ATTEMPTS ?? "60" });
if (manifest.version !== version) throw new Error("staged npm package does not match requested version");
if (![...PLATFORM_NAMES, "@tech-byte-frontier/hardgate"].includes(manifest.name)) throw new Error("unexpected staged npm package name");
const npmVersion = await runReleaseProcess("npm", ["--version"], { timeoutMs: childTimeoutMs(policy), env: auth.probeEnv });
validateNpmPublisherToolchain(auth.mode, { nodeVersion: process.version, npmVersion: npmVersion.trim() });
const request = { name: manifest.name, version, policy, env: auth.probeEnv };

async function publish() {
  await runReleaseProcess("npm", ["publish", "--provenance", "--access", "public", "--ignore-scripts", "--tag", "hardgate-candidate", directory], {
    timeoutMs: childTimeoutMs(policy),
    env: { ...auth.publishEnv, npm_config_registry: "https://registry.npmjs.org", npm_config_fetch_retries: "0" },
  });
}

async function verify() {
  const args = [path.join(projectRoot, "scripts/verify-npm-publication.mjs"), "--version", version, "--dist", path.resolve(option("--dist", "dist"))];
  if (manifest.name !== "@tech-byte-frontier/hardgate") args.push("--platform-only", "--package", manifest.name);
  const output = await runReleaseProcess(process.execPath, args, { timeoutMs: remainingMs(policy), env: auth.probeEnv });
  process.stdout.write(output);
}

const receipt = await publishVerifiedPackage(request, { probe: probeNpmVersion, publish, verify });
console.log(JSON.stringify(receipt));
