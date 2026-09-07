// Shared source loading and fixtures for the release safety contracts.
"use strict";

import { NATIVE_PACKAGES, PLATFORM_NAMES } from "../scripts/release-platforms.mjs";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => fs.readFileSync(path.join(root, relative), "utf8");

export const ci = read(".github/workflows/ci.yml");
export const release = read(".github/workflows/release.yml");
export const releaseAllowedSigners = read(".github/release-allowed-signers");
export const nodeVersion = read(".nvmrc").trim();
export const packageScript = read("scripts/release-package.mjs");
export const checksumScript = read("scripts/release-checksums.mjs");
export const verifier = read("scripts/release-verify.mjs");
export const releaseAbi = read("scripts/release-abi.mjs");
export const npmPublication = read("scripts/verify-npm-publication.mjs");
export const npmRegistryPack = read("scripts/npm-registry-pack.mjs");
export const npmVerificationPolicy = read("scripts/npm-verification-policy.mjs");
export const npmPackRetry = read("scripts/npm-pack-retry.mjs");
export const installedConsumers = read("scripts/release-consumers.sh");
export const directConsumer = read("scripts/release-direct-consumer.sh");
export const installedCheck = read("scripts/installed-check.mjs");
export const launcher = read("npm/hardgate/bin/hardgate.js");
export const sbomScript = read("scripts/release-sbom.mjs");
export const sbomVerifier = read("scripts/release-sbom-verify.mjs");
export const syncScript = read("scripts/sync-npm-version.mjs");
export const coverageScript = read("scripts/coverage.sh");
export const auditScript = read("scripts/dependency-audit.sh");
export const selfGate = read("scripts/self-gate.sh");
export const cargo = read("Cargo.toml");
export const rustToolchain = read("rust-toolchain.toml");
export const build = read("build.rs");
export const buildInfo = read("src/build_info.rs");

export const platformPackages = PLATFORM_NAMES;
export const targets = Object.values(NATIVE_PACKAGES).map(({target}) => target);

const npmRoot = path.join(root, "npm");
export const npmPlatformDirectories = fs
  .readdirSync(npmRoot, { withFileTypes: true })
  .filter((entry) => entry.isDirectory() && entry.name !== "hardgate")
  .map((entry) => entry.name)
  .sort();
export const wrapperManifest = JSON.parse(fs.readFileSync(path.join(npmRoot, "hardgate/package.json"), "utf8"));

export function includesAll(text, snippets, label) {
  for (const snippet of snippets) assert.ok(text.includes(snippet), `${label} must contain ${snippet}`);
}

export function releaseJob(name) {
  assert.match(name, /^[a-z][a-z0-9-]*$/);
  const start = release.indexOf(`  ${name}:\n`);
  assert.ok(start >= 0, `release job ${name} must exist`);
  const rest = release.slice(start);
  const next = rest.search(/\n  [a-z][a-z0-9-]*:\n/);
  return next < 0 ? rest : rest.slice(0, next);
}
