// Shared source loading and fixtures for the release safety contracts.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => fs.readFileSync(path.join(root, relative), "utf8");

export const ci = read(".github/workflows/ci.yml");
export const release = read(".github/workflows/release.yml");
export const releaseAllowedSigners = read(".github/release-allowed-signers");
export const installer = read("scripts/install.sh");
export const packageScript = read("scripts/release-package.mjs");
export const checksumScript = read("scripts/release-checksums.mjs");
export const verifier = read("scripts/release-verify.mjs");
export const releaseAbi = read("scripts/release-abi.mjs");
export const npmPublication = read("scripts/verify-npm-publication.mjs");
export const npmPackRetry = read("scripts/npm-pack-retry.mjs");
export const launcher = read("npm/hardgate/bin/hardgate.js");
export const sbomScript = read("scripts/release-sbom.mjs");
export const sbomVerifier = read("scripts/release-sbom-verify.mjs");
export const syncScript = read("scripts/sync-npm-version.mjs");
export const installerRuntime = read("tests/release_contract.install.test.mjs");
export const coverageScript = read("scripts/coverage.sh");
export const auditScript = read("scripts/dependency-audit.sh");
export const selfGate = read("scripts/self-gate.sh");
export const cargo = read("Cargo.toml");
export const build = read("build.rs");
export const buildInfo = read("src/build_info.rs");

export const platformPackages = [
  "hardgate-linux-x64",
  "hardgate-linux-x64-musl",
  "hardgate-linux-arm64",
  "hardgate-linux-arm64-musl",
  "hardgate-darwin-x64",
  "hardgate-darwin-arm64",
];
export const targets = [
  "x86_64-unknown-linux-gnu",
  "x86_64-unknown-linux-musl",
  "aarch64-unknown-linux-gnu",
  "aarch64-unknown-linux-musl",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
];

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
