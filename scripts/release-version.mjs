#!/usr/bin/env node
// Resolve and validate the version for a tagged release.
// Usage: node scripts/release-version.mjs --tag vX.Y.Z
"use strict";

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const platformPackages = [
  "hardgate-linux-x64",
  "hardgate-linux-x64-musl",
  "hardgate-linux-arm64",
  "hardgate-linux-arm64-musl",
  "hardgate-darwin-x64",
  "hardgate-darwin-arm64",
];
const semver = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

function argument(name) {
  const index = process.argv.indexOf(name);
  if (index >= 0) return process.argv[index + 1];
  const prefix = `${name}=`;
  return process.argv.find((value) => value.startsWith(prefix))?.slice(prefix.length);
}

function fail(message) {
  console.error(`release-version: ${message}`);
  process.exit(1);
}

function json(relative) {
  try {
    return JSON.parse(fs.readFileSync(path.join(root, relative), "utf8"));
  } catch (error) {
    fail(`cannot read ${relative}: ${error.message}`);
  }
}

function cargoVersion() {
  const text = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8");
  const version = text.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version || !semver.test(version)) fail("Cargo.toml has no valid package version");
  return version;
}

function tagVersion(tag) {
  if (!tag || !tag.startsWith("v")) fail(`release tag must be v<semver>, got ${tag || "<missing>"}`);
  const version = tag.slice(1);
  if (!semver.test(version)) fail(`release tag is not a semantic version: ${tag}`);
  return version;
}

function assertVersion(label, value, expected) {
  if (value !== expected) fail(`${label}=${value || "<missing>"} does not match tagged version ${expected}`);
}

const tag = argument("--tag");
const version = tagVersion(tag);
assertVersion("Cargo.toml package.version", cargoVersion(), version);

const lock = fs.readFileSync(path.join(root, "Cargo.lock"), "utf8");
const lockVersion = lock.match(/\[\[package\]\]\s+name = "hardgate"\s+version = "([^"]+)"/m)?.[1];
assertVersion("Cargo.lock hardgate.version", lockVersion, version);

const workspace = json("package.json");
assertVersion("package.json version", workspace.version, version);
const main = json("npm/hardgate/package.json");
assertVersion("npm/hardgate version", main.version, version);

const actualPackages = fs
  .readdirSync(path.join(root, "npm"), { withFileTypes: true })
  .filter((entry) => entry.isDirectory() && entry.name !== "hardgate")
  .map((entry) => entry.name)
  .sort();
if (actualPackages.join("\n") !== [...platformPackages].sort().join("\n")) {
  fail(`platform package directories must be exactly ${platformPackages.join(", ")}`);
}

const optional = Object.keys(main.optionalDependencies ?? {}).sort();
if (optional.join("\n") !== [...platformPackages].sort().join("\n")) {
  fail("main npm wrapper optionalDependencies advertise an unsupported platform");
}
for (const packageName of platformPackages) {
  const packageJson = json(`npm/${packageName}/package.json`);
  assertVersion(`${packageName}.version`, packageJson.version, version);
  assertVersion(`optionalDependencies[${packageName}]`, main.optionalDependencies[packageName], version);
}

console.log(version);
